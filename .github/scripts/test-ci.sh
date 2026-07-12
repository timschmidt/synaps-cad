#!/usr/bin/env bash
set -euo pipefail

# The external OpenSCAD corpus and these inline stress models are compiled with
# the test binary but run separately during compatibility/performance work.
cargo test --locked --release -- \
  --test-threads=1 \
  --skip compiler::evaluator::tests::openscad_ \
  --skip compiler::evaluator::tests::test_candle_stand \
  --skip compiler::evaluator::tests::test_intersection_sphere_cube \
  --skip compiler::evaluator::tests::test_refill_clip \
  --skip compiler::evaluator::tests::test_star_difference \
  --skip compiler::evaluator::tests::test_text_difference_on_cube
