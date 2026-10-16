// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The compile lifecycle: FORM evaluation off the UI thread with startup and execution budgets. Ported from
//! `compiler-protocol.ts` and `compiler-worker.ts`.
//!
//! Each request gets its own job, as the TypeScript tool starts a fresh worker per build; starting another request or
//! cancelling abandons the current job and ignores anything it reports later. Native builds evaluate on a background
//! thread. Browsers evaluate inline when the job is polled (WebAssembly threads would need cross-origin isolation),
//! within the same execution budget. FORM's own operation, vertex, and allocation limits bound every evaluation, so an
//! abandoned native job always finishes; its result is dropped. Callers keep the last accepted geometry whenever a job
//! fails, times out, or is abandoned.

use joy_engine::form::{Diagnostic, GeometryData};
use joy_engine::runtime::Stopwatch;
use joy_engine::serde_json::Value as Json;

/// Budget for a job to begin evaluating (the TypeScript worker's module-loading window), in milliseconds.
pub const STARTUP_TIMEOUT_MILLISECONDS: f64 = 10_000.0;

/// Budget for evaluating one script, in milliseconds.
pub const EXECUTION_TIMEOUT_MILLISECONDS: f64 = 2_000.0;

/// What one build evaluates: the script, its parameter overrides, and the document material palette.
#[derive(Clone, Debug, PartialEq)]
pub struct CompileRequest {
    /// FORM source.
    pub source: String,
    /// A JSON object of parameter overrides.
    pub overrides: Json,
    /// The document's material palette (a JSON array), or `None` for the Default material alone.
    pub materials: Option<Json>,
}

/// How a build ended.
#[derive(Clone, Debug, PartialEq)]
pub enum CompileOutcome {
    /// The script evaluated within budget.
    Built {
        /// The generated geometry.
        data: Box<GeometryData>,
        /// Evaluation time in milliseconds.
        milliseconds: f64,
    },
    /// The script was rejected, with the located diagnostic.
    Rejected(Diagnostic),
    /// The job did not begin evaluating within [`STARTUP_TIMEOUT_MILLISECONDS`].
    StartupExpired,
    /// Evaluation exceeded [`EXECUTION_TIMEOUT_MILLISECONDS`].
    ExecutionExpired,
    /// The job could not be started or stopped unexpectedly.
    Failed(String),
}

impl CompileOutcome {
    /// The status line the TypeScript tool shows for an unsuccessful outcome (successes return `None`).
    pub fn failure_message(&self) -> Option<String> {
        match self {
            Self::Built { .. } => None,
            Self::Rejected(diagnostic) => Some(format!(
                "Line {}, column {}: {} Previous preview retained.",
                diagnostic.line, diagnostic.column, diagnostic.message
            )),
            Self::StartupExpired => Some(
                "Compiler startup exceeded 10 seconds. Check module loading or restart the tool. Previous preview \
                 retained."
                    .into(),
            ),
            Self::ExecutionExpired => {
                Some("Build exceeded 2 seconds. Reduce loops or resolution. Previous preview retained.".into())
            }
            Self::Failed(message) => Some(format!("Compiler could not start: {message}. Previous preview retained.")),
        }
    }
}

/// Where the current job is in its lifecycle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CompilePhase {
    /// No job is running.
    #[default]
    Idle,
    /// The job was requested and has not begun evaluating.
    Starting,
    /// The job is evaluating.
    Compiling,
}

/// Runs at most one build at a time and reports its outcome when polled.
#[derive(Default)]
pub struct Compiler {
    job: Option<Job>,
    /// Budgets, replaceable for tests.
    budgets: Budgets,
}

/// Startup and execution budgets in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Budgets {
    /// See [`STARTUP_TIMEOUT_MILLISECONDS`].
    pub startup_milliseconds: f64,
    /// See [`EXECUTION_TIMEOUT_MILLISECONDS`].
    pub execution_milliseconds: f64,
}

impl Default for Budgets {
    fn default() -> Self {
        Self {
            startup_milliseconds: STARTUP_TIMEOUT_MILLISECONDS,
            execution_milliseconds: EXECUTION_TIMEOUT_MILLISECONDS,
        }
    }
}

impl Compiler {
    /// A compiler with the standard budgets.
    pub fn new() -> Self {
        Self::default()
    }

    /// A compiler with custom budgets (tests use short ones).
    pub fn with_budgets(budgets: Budgets) -> Self {
        Self { job: None, budgets }
    }

    /// Start a build, abandoning any current one.
    pub fn start(&mut self, request: CompileRequest) {
        self.job = Some(Job::spawn(request));
    }

    /// Abandon the current build without an outcome.
    pub fn cancel(&mut self) {
        self.job = None;
    }

    /// The current job's phase.
    pub fn phase(&self) -> CompilePhase {
        match &self.job {
            None => CompilePhase::Idle,
            Some(job) if job.compiling.is_some() => CompilePhase::Compiling,
            Some(_) => CompilePhase::Starting,
        }
    }

    /// Advance the current job; returns its outcome once it ends (the job is then gone).
    pub fn poll(&mut self) -> Option<CompileOutcome> {
        let budgets = self.budgets;
        let job = self.job.as_mut()?;
        let outcome = job.poll(budgets);
        if outcome.is_some() {
            self.job = None;
        }
        outcome
    }

    /// Poll until the current job ends, sleeping briefly between polls natively. Returns `None` without a job.
    pub fn finish(&mut self) -> Option<CompileOutcome> {
        loop {
            self.job.as_ref()?;
            if let Some(outcome) = self.poll() {
                return Some(outcome);
            }
            #[cfg(not(target_arch = "wasm32"))]
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

/// Evaluate a request synchronously, as the worker does, returning the outcome and its evaluation time.
pub fn evaluate(request: &CompileRequest) -> (Result<GeometryData, Diagnostic>, f64) {
    let stopwatch = Stopwatch::start();
    let result = joy_engine::form::compile(&request.source, &request.overrides, request.materials.as_ref());
    (result, stopwatch.elapsed_milliseconds())
}

/// One build's messages, mirroring the worker protocol: a compiling notice, then a result.
enum Message {
    Compiling,
    Finished(Box<Result<GeometryData, Diagnostic>>, f64),
}

/// One running build.
struct Job {
    requested: Stopwatch,
    compiling: Option<Stopwatch>,
    #[cfg(not(target_arch = "wasm32"))]
    receiver: std::sync::mpsc::Receiver<Message>,
    #[cfg(target_arch = "wasm32")]
    request: Option<CompileRequest>,
}

impl Job {
    #[cfg(not(target_arch = "wasm32"))]
    fn spawn(request: CompileRequest) -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("form-lab-compiler".into()).spawn(move || {
            // A dropped receiver means the job was abandoned; there is nothing to report to.
            let _ = sender.send(Message::Compiling);
            let (result, milliseconds) = evaluate(&request);
            let _ = sender.send(Message::Finished(Box::new(result), milliseconds));
        });
        if let Err(error) = spawned {
            log_spawn_failure(&error.to_string());
        }
        Self { requested: Stopwatch::start(), compiling: None, receiver }
    }

    #[cfg(target_arch = "wasm32")]
    fn spawn(request: CompileRequest) -> Self {
        Self { requested: Stopwatch::start(), compiling: None, request: Some(request) }
    }

    fn poll(&mut self, budgets: Budgets) -> Option<CompileOutcome> {
        loop {
            match self.receive() {
                Ok(Some(Message::Compiling)) => self.compiling = Some(Stopwatch::start()),
                Ok(Some(Message::Finished(result, milliseconds))) => {
                    // Result delivery may wait behind the UI thread; evaluation time owns the budget.
                    return Some(match *result {
                        Ok(_) if milliseconds > budgets.execution_milliseconds => CompileOutcome::ExecutionExpired,
                        Ok(data) => CompileOutcome::Built { data: Box::new(data), milliseconds },
                        Err(diagnostic) => CompileOutcome::Rejected(diagnostic),
                    });
                }
                Ok(None) => break,
                Err(message) => return Some(CompileOutcome::Failed(message)),
            }
        }
        match self.compiling {
            Some(compiling) if compiling.elapsed_milliseconds() > budgets.execution_milliseconds => {
                Some(CompileOutcome::ExecutionExpired)
            }
            None if self.requested.elapsed_milliseconds() > budgets.startup_milliseconds => {
                Some(CompileOutcome::StartupExpired)
            }
            _ => None,
        }
    }

    /// The next message, `None` when nothing is pending, or an error when the job stopped without a result.
    #[cfg(not(target_arch = "wasm32"))]
    fn receive(&mut self) -> Result<Option<Message>, String> {
        match self.receiver.try_recv() {
            Ok(message) => Ok(Some(message)),
            Err(std::sync::mpsc::TryRecvError::Empty) => Ok(None),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("the compiler stopped unexpectedly".into()),
        }
    }

    /// Browsers evaluate inline on the first poll: a compiling notice, then the result on the next read.
    #[cfg(target_arch = "wasm32")]
    fn receive(&mut self) -> Result<Option<Message>, String> {
        if self.compiling.is_none() {
            return Ok(Some(Message::Compiling));
        }
        Ok(self.request.take().map(|request| {
            let (result, milliseconds) = evaluate(&request);
            Message::Finished(Box::new(result), milliseconds)
        }))
    }
}

/// Report a thread that could not start; the job then ends with [`CompileOutcome::Failed`] on its next poll.
#[cfg(not(target_arch = "wasm32"))]
fn log_spawn_failure(message: &str) {
    joy_engine::log::error!("FORM LAB could not start its compiler thread: {message}");
}
