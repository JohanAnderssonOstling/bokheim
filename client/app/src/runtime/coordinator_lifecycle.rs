//! Coordinator lifecycle decisions, independent of browser workers and ports.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Connect { id: u32 },
    Reject { id: u32, error: String },
    StopBackground,
    FailClients { error: String },
    FailNetwork { error: String },
    FailSync { error: String },
    FailCpu { error: String },
    NotifySyncFailure { error: String },
    StopNetwork,
    StopBackend,
    Close,
}

#[derive(Default)]
enum State {
    #[default]
    Starting,
    Ready,
    Failed(String),
}

#[derive(Default)]
pub struct CoordinatorLifecycle {
    state: State,
    waiting: Vec<u32>,
}

impl CoordinatorLifecycle {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn starting(&self) -> bool {
        matches!(self.state, State::Starting)
    }
    pub fn accepting_work(&self) -> bool {
        !matches!(self.state, State::Failed(_))
    }

    pub fn connect(&mut self, id: u32) -> Vec<Action> {
        match &self.state {
            State::Starting => {
                self.waiting.push(id);
                vec![]
            }
            State::Ready => vec![Action::Connect { id }],
            State::Failed(error) => vec![Action::Reject { id, error: error.clone() }],
        }
    }
    pub fn ready(&mut self) -> Vec<Action> {
        if !matches!(self.state, State::Starting) {
            return vec![];
        }
        self.state = State::Ready;
        self.waiting.drain(..).map(|id| Action::Connect { id }).collect()
    }
    pub fn fail(&mut self, error: String) -> Vec<Action> {
        if matches!(self.state, State::Failed(_)) {
            return vec![];
        }
        self.state = State::Failed(error.clone());
        let mut result = vec![
            Action::StopBackground,
            Action::FailClients { error: error.clone() },
            Action::FailNetwork { error: error.clone() },
            Action::FailSync { error: error.clone() },
            Action::NotifySyncFailure { error: error.clone() },
            Action::FailCpu { error: error.clone() },
            Action::StopNetwork,
        ];
        result.extend(self.waiting.drain(..).map(|id| Action::Reject { id, error: error.clone() }));
        // Release SQLite/OPFS ownership before closing the coordinator.
        result.extend([Action::StopBackend, Action::Close]);
        result
    }
}
