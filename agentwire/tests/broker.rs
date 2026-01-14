use agentwire::{
    agent::{Cell, Task},
    port::{self, Port},
    Agent, Broker, BrokerFlow,
};
use futures::FutureExt;

#[derive(Debug, thiserror::Error)]
#[error("dummy error")]
pub struct Error;

struct DummyAgent;

impl Port for DummyAgent {
    type Input = ();
    type Output = ();

    const INPUT_CAPACITY: usize = 0;
    const OUTPUT_CAPACITY: usize = 0;
}

impl Agent for DummyAgent {
    const NAME: &'static str = "dummy";
}

impl Task for DummyAgent {
    type Error = Error;

    async fn run(self, _port: agentwire::port::Inner<Self>) -> Result<(), Self::Error> {
        std::future::pending().await
    }
}

#[derive(Broker)]
#[broker(plan = PlanT, error = Error)]
struct NoAgents {}

impl NoAgents {
    fn new() -> Self {
        new_no_agents!()
    }
}

#[derive(Broker)]
#[broker(plan = PlanT, error = Error)]
struct OneAgent {
    #[agent(task, init)]
    dummy: Cell<DummyAgent>,
}

impl OneAgent {
    fn new() -> Self {
        new_one_agent!()
    }

    #[expect(dead_code, reason = "agent never enabled")]
    fn init_dummy(&mut self) -> DummyAgent {
        DummyAgent
    }

    fn handle_dummy(
        &mut self,
        _plan: &mut dyn PlanT,
        _output: port::Output<DummyAgent>,
    ) -> Result<BrokerFlow, Error> {
        unreachable!("agent never enabled")
    }
}

trait PlanT {}

#[derive(Debug)]
struct Plan;

impl PlanT for Plan {}

#[test]
fn test_broker_with_no_agents_never_blocks() {
    let waker = futures::task::noop_waker();
    let mut cx = std::task::Context::from_waker(&waker);
    let mut plan = Plan;

    let mut no_agents = NoAgents::new();
    let mut run_fut = no_agents.run(&mut plan);
    // poll should always immediately return, instead of looping forever.
    assert!(run_fut.poll_unpin(&mut cx).is_pending());
    assert!(run_fut.poll_unpin(&mut cx).is_pending());
    assert!(run_fut.poll_unpin(&mut cx).is_pending());
}

#[test]
fn test_broker_with_one_agent_never_blocks() {
    let waker = futures::task::noop_waker();
    let mut cx = std::task::Context::from_waker(&waker);
    let mut plan = Plan;

    let mut one_agent = OneAgent::new();
    let mut run_fut = one_agent.run(&mut plan);
    // poll should always immediately return, instead of looping forever.
    assert!(run_fut.poll_unpin(&mut cx).is_pending());
    assert!(run_fut.poll_unpin(&mut cx).is_pending());
    assert!(run_fut.poll_unpin(&mut cx).is_pending());
}
