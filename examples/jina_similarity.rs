use std::collections::VecDeque;
use std::error::Error;
use std::fs;

use wasnn::ops::concat;
use wasnn::{FloatOperators, Input, Model, NodeId, Operators};
use wasnn_tensor::prelude::*;
use wasnn_tensor::{NdTensor, Tensor};
use wasnn_text::normalizer::{Normalizer, NormalizerOptions};
use wasnn_text::tokenizers::{EncodeOptions, Tokenizer, WordPiece, WordPieceOptions};

struct Args {
    model: String,
    vocab: String,
    index_file: String,
    query: String,

    #[allow(dead_code)]
    verbose: bool,
}

fn parse_args() -> Result<Args, lexopt::Error> {
    use lexopt::prelude::*;

    let mut values = VecDeque::new();
    let mut parser = lexopt::Parser::from_env();
    let mut verbose = false;

    while let Some(arg) = parser.next()? {
        match arg {
            Value(val) => values.push_back(val.string()?),
            Short('v') | Long("verbose") => verbose = true,
            Long("help") => {
                println!(
                    "Estimate semantic similarity of two sentences.

Usage: {bin_name} <model> <vocab> <index_file> <query>

Args:

  <model>       - Input model
  <vocab>       - Vocabulary for tokenization (vocab.txt)
  <index_file>  - File containing sentences to search (one per line)
  <query>       - Sentence to match against index file

Options:

  -v, --verbose - Output debug info
",
                    bin_name = parser.bin_name().unwrap_or("jina_similarity")
                );
                std::process::exit(0);
            }
            _ => return Err(arg.unexpected()),
        }
    }

    let model = values.pop_front().ok_or("missing `model` arg")?;
    let vocab = values.pop_front().ok_or("missing `vocab` arg")?;
    let index_file = values.pop_front().ok_or("missing `index_file` arg")?;
    let query = values.pop_front().ok_or("missing `query` arg")?;

    let args = Args {
        model,
        vocab,
        index_file,
        query,
        verbose,
    };

    Ok(args)
}

/// Generate embeddings for a slice of sentences.
///
/// Returns a `(batch, embed_dim)` tensor where `batch` is equal to `sentences.len()`.
fn embed_sentence_batch(
    sentences: &[&str],
    tokenizer: &WordPiece,
    model: &Model,
    max_seq_len: usize,
) -> Result<NdTensor<f32, 2>, Box<dyn Error>> {
    // Tokenize input sequences
    let mut encoded = Vec::new();
    for &sentence in sentences {
        encoded.push(tokenizer.encode(
            sentence.into(),
            EncodeOptions {
                max_chunk_len: Some(max_seq_len),
                ..Default::default()
            },
        )?);
    }

    // Generate (batch, token_id) input.
    let max_sequence_len = encoded
        .iter()
        .map(|enc| enc.token_ids().len())
        .max()
        .unwrap_or(0);
    let batch = sentences.len();
    let mut input_ids = Tensor::zeros(&[batch, max_sequence_len]);
    for (i, encoded) in encoded.iter().enumerate() {
        let token_ids = encoded.token_ids();
        for (tid, input_id) in token_ids
            .iter()
            .zip(input_ids.slice_mut((i, ..token_ids.len())).iter_mut())
        {
            *input_id = *tid as i32;
        }
    }

    // Generate attention mask, set to 1 for non-padding tokens and 0 for
    // padding tokens.
    let mut attention_mask = Tensor::zeros(&[batch, max_sequence_len]);
    for (i, encoded) in encoded.iter().enumerate() {
        attention_mask
            .slice_mut((i, ..encoded.token_ids().len()))
            .fill(1i32);
    }

    let input_ids_id = model.node_id("input_ids")?;
    let attention_mask_id = model.node_id("attention_mask")?;

    let mut inputs: Vec<(NodeId, Input)> = vec![
        (input_ids_id, input_ids.view().into()),
        (attention_mask_id, attention_mask.view().into()),
    ];

    // Generate token type IDs if this model needs them. These are all zeros
    // since each item has just one sequence.
    let type_ids: Tensor<i32>;
    if let Some(type_ids_id) = model.find_node("token_type_ids") {
        type_ids = Tensor::zeros(&[batch, max_sequence_len]);
        inputs.push((type_ids_id, type_ids.view().into()));
    }

    let output_id = model.node_id("last_hidden_state")?;
    let [last_hidden_state] = model.run_n(&inputs, [output_id], None)?;
    let last_hidden_state = last_hidden_state.into_float().ok_or("wrong output type")?;

    // Mean pool each item in the batch. We process each batch item separately
    // since they can have different lengths.
    let mean_pooled: Vec<_> = last_hidden_state
        .axis_iter(0)
        .zip(encoded.iter())
        .map(|(item, input)| {
            // Take the mean of the non-padding elements along the sequence
            // dimension.
            let seq_len = input.token_ids().len();
            item.slice(..seq_len)
                .reduce_mean(Some(&[0]), false /* keep_dims */)
                .unwrap()
        })
        .collect();
    let mean_pooled_views: Vec<_> = mean_pooled
        .iter()
        .map(|mp| {
            // Re-add batch dim.
            let mut view = mp.view();
            view.insert_dim(0);
            view
        })
        .collect();
    let mean_pooled: NdTensor<f32, 2> = concat(&mean_pooled_views, 0)?.try_into()?;
    Ok(mean_pooled)
}

/// This example computes the semantic similarity between a query sentence and
/// a list of sentences in a text file (one per line).
///
/// It uses the Jina embeddings model from
/// https://huggingface.co/jinaai/jina-embeddings-v2-small-en. You can download
/// the in ONNX format, along with the vocab.txt vocabulary file from
/// https://huggingface.co/jinaai/jina-embeddings-v2-small-en/tree/main.
///
/// Convert the model using:
///
/// ```text
/// tools/convert-onnx.py jina-embed.onnx jina-embed.model
/// ```
///
/// Then run the example with:
///
/// ```text
/// cargo run -r --example jina_similarity jina-embed.model jina-vocab.txt
///   examples/data/rust-questions.txt "How can I make a function work with any type that supports addition?"
/// ```
///
/// This should output a result such as:
///
/// ```text
/// Query: "How can I make a function work with any type that supports addition?"
///
/// Best matches:
///   #1: How do I require a generic type implement an operation like Add, Sub, Mul, or Div in a generic function?
///   #2: How do I implement a trait I don't own for a type I don't own?
///   #3: Is there any way to return a reference to a variable created in a function?
///   ...
/// ```
fn main() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;
    let model_bytes = fs::read(args.model)?;
    let model = Model::load(&model_bytes)?;

    let vocab_text = std::fs::read_to_string(&args.vocab)?;
    let vocab: Vec<_> = vocab_text.lines().collect();

    let normalizer = Normalizer::new(NormalizerOptions {
        lowercase: true,
        strip_accents: true,
        ..Default::default()
    });
    let tokenizer = WordPiece::from_vocab(
        &vocab,
        WordPieceOptions {
            normalizer: Some(normalizer),
            ..Default::default()
        },
    );

    let mut sentences: Vec<&str> = vec![&args.query];

    // Read sentences from index. We only use the first 50 lines to limit the
    // runtime in this example. Here we recompute embeddings on every run, but
    // in an actual app you'd want to precompute the embeddings. Also you'd want
    // to chunk up the documents/sentences to index into batches (eg. of 16,
    // 32...).
    let index_content = std::fs::read_to_string(&args.index_file)?;
    sentences.extend(index_content.lines().take(50));

    // Max sequence length supported by Jina embeddings.
    // See notes in https://huggingface.co/jinaai/jina-embeddings-v2-base-en.
    let max_sequence_len = 8192;

    // (batch, embed_dim) matrix of embeddings.
    let embeddings = embed_sentence_batch(&sentences, &tokenizer, &model, max_sequence_len)?;

    // Compute cosine similarity of first row in `embeddings` with all rows.
    //
    // First normalize embeddings to unit length, then compute the dot product
    // of the first row with other rows.
    let embeddings_norm =
        embeddings.reduce_l2(Some(&[1]) /* axes */, true /* keep_dims */)?;
    let embeddings: NdTensor<f32, 2> = embeddings.div(embeddings_norm.view())?.try_into()?;

    // (1, embed) @ (embed, batch) => (1, batch)
    let similarities = embeddings
        .slice::<2, _>(..1)
        .matmul(embeddings.as_dyn().transposed())?;

    // Sort results by similarity to the query.
    //
    // Note that the raw scores are not very meaningful by themselves and will
    // all be "high" values (close to 1.0). They should be used only for
    // comparison with other scores.
    let mut scores: Vec<(usize, f32)> = similarities
        .slice(0)
        .iter()
        .copied()
        .enumerate()
        .skip(1) // Skip over similarity of query with itself.
        .collect();
    scores.sort_by(|(_idx_a, score_a), (_idx_b, score_b)| score_a.total_cmp(score_b).reverse());

    println!("Query: \"{}\"", sentences[0]);
    println!("");
    println!("Best matches:");
    for (rank, (idx, _score)) in scores.iter().take(10).enumerate() {
        println!("  #{}: {}", rank + 1, sentences[*idx]);
    }

    Ok(())
}
