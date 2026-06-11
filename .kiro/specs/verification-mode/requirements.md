# Requirements Document

## Introduction

Phase 2.4 adds an optional dual-compute verification system to the Computation-Addressed Distributed File System (Deriva/CAS-DFS). The system's core invariant is computation addressing: the same recipe (function + inputs + params) MUST always produce the same output. Verification mode detects violations of this invariant by executing compute functions twice in parallel and comparing outputs byte-for-byte. The system supports three configurable modes: off (production default), dual-compute (always verify), and sampled (deterministic fraction-based verification). This enables development-time bug detection, CI/CD gating, and production monitoring with minimal wall-clock overhead due to parallel execution.

## Glossary

- **Executor**: The AsyncExecutor component responsible for materializing computation-addressed nodes by resolving inputs, executing compute functions, and caching results.
- **VerificationMode**: An enumeration controlling verification behavior — Off (single execution), DualCompute (parallel dual execution), or Sampled (fraction-based deterministic verification).
- **DeterminismViolation**: An error variant raised when dual-compute detects that a compute function produced different outputs for identical inputs.
- **VerificationStats**: A statistics tracker using atomic counters for total_verified, total_passed, and total_failed verification attempts, plus last failure storage.
- **CAddr**: A content address (blake3 hash) uniquely identifying a node in the computation DAG.
- **Recipe**: A computation specification containing a function_id, input addresses, and parameters.
- **ComputeFunction**: A registered function that transforms input bytes and parameters into output bytes. Must be deterministic.
- **Sampling_Rate**: A floating-point value between 0.0 and 1.0 controlling what fraction of recipes are verified in Sampled mode.
- **Status_RPC**: A gRPC endpoint that returns server state including verification statistics.
- **Verify_RPC**: A gRPC endpoint that performs on-demand dual-compute verification for a specific recipe address.

## Requirements

### Requirement 1: Verification Mode Configuration

**User Story:** As a system operator, I want to configure the verification mode at server startup, so that I can balance between determinism assurance and performance cost.

#### Acceptance Criteria

1. THE Executor SHALL support three verification modes: Off, DualCompute, and Sampled with a rate parameter.
2. THE Executor SHALL default to Off mode when no verification configuration is specified.
3. WHEN the server starts with `--verification off`, THE Executor SHALL operate in Off mode with single execution per recipe.
4. WHEN the server starts with `--verification dual`, THE Executor SHALL operate in DualCompute mode executing each recipe twice.
5. WHEN the server starts with `--verification sampled:RATE` where RATE is a decimal between 0.0 and 1.0, THE Executor SHALL operate in Sampled mode with the specified rate.
6. IF an invalid verification mode string is provided at startup, THEN THE Server SHALL reject the configuration and fail to start with a descriptive error message.

### Requirement 2: Single Execution (Off Mode)

**User Story:** As a production operator, I want zero-overhead execution when verification is disabled, so that production workloads run at maximum performance.

#### Acceptance Criteria

1. WHILE the Executor is in Off mode, THE Executor SHALL execute each compute function exactly once per materialization request.
2. WHILE the Executor is in Off mode, THE Executor SHALL execute the compute function via a single `spawn_blocking` call.
3. WHILE the Executor is in Off mode, THE Executor SHALL not increment any verification statistics counters.

### Requirement 3: Dual-Compute Verification

**User Story:** As a developer, I want all recipes verified via dual execution, so that I can detect any non-deterministic compute functions during development and testing.

#### Acceptance Criteria

1. WHILE the Executor is in DualCompute mode, THE Executor SHALL execute each compute function exactly twice per materialization request.
2. WHILE the Executor is in DualCompute mode, THE Executor SHALL execute both instances in parallel using `tokio::join!` with separate `spawn_blocking` calls.
3. WHEN both executions produce byte-identical output, THE Executor SHALL return the first output as the materialization result.
4. WHEN both executions produce byte-identical output, THE Executor SHALL record a pass in VerificationStats.
5. WHEN the two executions produce different output, THE Executor SHALL return a DeterminismViolation error.
6. WHEN the two executions produce different output, THE Executor SHALL record a failure in VerificationStats including the violation details.

### Requirement 4: Deterministic Sampled Verification

**User Story:** As a production operator, I want to verify a configurable fraction of recipes with deterministic selection, so that I can monitor for non-determinism in production with predictable, reproducible behavior.

#### Acceptance Criteria

1. WHILE the Executor is in Sampled mode, THE Executor SHALL determine whether to verify a recipe using the formula `(addr.as_bytes()[0] as f64) / 255.0 < rate`.
2. WHILE the Executor is in Sampled mode, THE Executor SHALL always make the same verification decision for the same CAddr across multiple materialization attempts.
3. WHEN the sampling decision is true for a given CAddr, THE Executor SHALL perform dual-compute verification identical to DualCompute mode.
4. WHEN the sampling decision is false for a given CAddr, THE Executor SHALL perform single execution identical to Off mode.
5. WHEN the sampling rate is 0.0, THE Executor SHALL never verify any recipe.
6. WHEN the sampling rate is 1.0, THE Executor SHALL verify every recipe.

### Requirement 5: DeterminismViolation Error

**User Story:** As a developer debugging non-determinism, I want detailed error information when a violation is detected, so that I can identify and fix the offending function.

#### Acceptance Criteria

1. WHEN a DeterminismViolation is detected, THE Executor SHALL include the CAddr of the failing recipe in the error.
2. WHEN a DeterminismViolation is detected, THE Executor SHALL include the function_id of the compute function in the error.
3. WHEN a DeterminismViolation is detected, THE Executor SHALL include blake3 hashes of both outputs in the error.
4. WHEN a DeterminismViolation is detected, THE Executor SHALL include the byte lengths of both outputs in the error.
5. THE DeterminismViolation error message SHALL format as: `determinism violation for {addr}: function {function_id} produced different outputs ({output_1_len} bytes hash={output_1_hash} vs {output_2_len} bytes hash={output_2_hash})`.

### Requirement 6: Verification Statistics

**User Story:** As an operator monitoring system health, I want cumulative verification statistics, so that I can track the determinism compliance rate over time.

#### Acceptance Criteria

1. THE VerificationStats SHALL maintain atomic counters for total_verified, total_passed, and total_failed.
2. WHEN a verification pass occurs, THE VerificationStats SHALL atomically increment total_verified and total_passed.
3. WHEN a verification failure occurs, THE VerificationStats SHALL atomically increment total_verified and total_failed.
4. WHEN a verification failure occurs, THE VerificationStats SHALL store the DeterminismViolation as the last_failure.
5. THE VerificationStats SHALL provide a failure_rate() method returning `total_failed / total_verified`, or 0.0 when total_verified is zero.

### Requirement 7: Verification Scope

**User Story:** As a system designer, I want verification to apply only to the compute step, so that determinism checking targets the source of potential non-determinism without redundant work.

#### Acceptance Criteria

1. THE Executor SHALL apply verification only to the `func.execute()` compute step within materialization.
2. WHEN a materialization request hits the cache, THE Executor SHALL return the cached result without performing any verification.
3. WHEN a materialization request resolves a leaf node, THE Executor SHALL return the leaf data without performing any verification.
4. WHEN a recipe has multiple inputs, THE Executor SHALL verify each input's compute step independently during recursive materialization.

### Requirement 8: Status RPC Enhancement

**User Story:** As an operator, I want the Status RPC to include verification statistics, so that I can monitor verification health via standard server status queries.

#### Acceptance Criteria

1. THE Status_RPC response SHALL include a `verification_mode` field indicating the current mode as a string ("off", "dual", or "sampled:RATE").
2. THE Status_RPC response SHALL include a `verification_total` field with the count of total verification attempts.
3. THE Status_RPC response SHALL include a `verification_passed` field with the count of passing verifications.
4. THE Status_RPC response SHALL include a `verification_failed` field with the count of failed verifications.
5. THE Status_RPC response SHALL include a `verification_failure_rate` field with the computed failure rate as a floating-point value.

### Requirement 9: On-Demand Verify RPC

**User Story:** As an operator or CI system, I want to verify a specific recipe on demand, so that I can spot-check critical recipes or validate after function updates without enabling global verification.

#### Acceptance Criteria

1. WHEN a Verify RPC is received with a valid recipe address, THE Server SHALL perform dual-compute verification for that recipe regardless of the configured verification mode.
2. WHEN verification succeeds, THE Verify_RPC response SHALL include `deterministic: true`, the output hash, the output size, and compute time in microseconds.
3. WHEN verification detects non-determinism, THE Verify_RPC response SHALL include `deterministic: false` and a descriptive error string.
4. IF the specified address does not exist in the DAG, THEN THE Server SHALL return a not-found status error.
5. IF any input resolution fails during verification, THEN THE Server SHALL return an internal error with the resolution failure details.

### Requirement 10: Parallel Execution Performance

**User Story:** As a system architect, I want dual-compute to run both executions in parallel, so that wall-clock overhead remains minimal despite the 2x CPU cost.

#### Acceptance Criteria

1. WHILE in DualCompute mode, THE Executor SHALL execute both function instances concurrently rather than sequentially.
2. THE Executor SHALL use separate `spawn_blocking` tasks for each execution to leverage thread-pool parallelism.
3. WHILE in DualCompute mode, THE Executor wall-clock overhead compared to Off mode SHALL remain under 20% for typical compute functions.
