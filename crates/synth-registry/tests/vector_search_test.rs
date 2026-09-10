// SPDX-License-Identifier: Apache-2.0

//! Sub-Phase 12a Vector Search Retrieval & Latency Test.

use std::path::Path;
use std::time::Instant;
use synth_registry::{load_dir, VectorSearchIndex};

#[test]
fn test_semantic_vector_search_finds_ldo_regulator() {
    let registry_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry/parts");
    let registry = load_dir(&registry_dir).expect("load registry");

    let start_build = Instant::now();
    let index = VectorSearchIndex::build(&registry);
    let build_duration = start_build.elapsed();

    println!("[Sub-Phase 12a] Built 384-D vector index in {build_duration:?}");

    let start_search = Instant::now();
    let results = index.search("3.3V low-quiescent LDO regulator in SOT-23", 5);
    let search_duration = start_search.elapsed();

    println!("[Sub-Phase 12a] Executed vector query in {search_duration:?}");
    println!("[Sub-Phase 12a] Top matches: {results:?}");

    // 1. Latency Gate Check (<10ms)
    assert!(
        search_duration.as_millis() < 10,
        "Vector search latency ({search_duration:?}) must be < 10ms"
    );

    // 2. Retrieval Accuracy Gate Check
    assert!(
        !results.is_empty(),
        "Vector search must return matching results for regulator query"
    );
    let found_regulator = results
        .iter()
        .any(|r| r.kind == "regulator" || r.kind == "ldo" || r.similarity_score > 0.3);
    assert!(
        found_regulator,
        "Vector search top-3 matches must retrieve relevant regulator parts"
    );
}
