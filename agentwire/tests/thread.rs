use agentwire::{
    agent,
    port::{self, Port},
    Agent, Broker, BrokerFlow,
};
use futures::{channel::mpsc::SendError, prelude::*};
use std::{io, time::Instant};
use thiserror::Error;
use tokio::runtime;

#[derive(Default)]
struct Doubler;

impl Port for Doubler {
    type Input = u32;
    type Output = u32;

    const INPUT_CAPACITY: usize = 0;
    const OUTPUT_CAPACITY: usize = 0;
}

impl Agent for Doubler {
    const NAME: &'static str = "doubler";
}

#[derive(Error, Debug)]
pub enum DoublerError {
    #[error("tokio runtime error")]
    Runtime(#[from] io::Error),
    #[error("send error")]
    Send(#[from] SendError),
}

impl agent::Thread for Doubler {
    type Error = DoublerError;

    fn run(self, mut port: port::Inner<Self>) -> Result<(), Self::Error> {
        let rt = runtime::Builder::new_current_thread().enable_all().build()?;
        while let Some(x) = rt.block_on(port.next()) {
            rt.block_on(port.send(x.chain(x.value * 2)))?;
        }
        Ok(())
    }
}

#[derive(Error, Debug)]
pub enum Error {}

trait Plan {
    fn handle_doubler(
        &mut self,
        broker: &mut Broker,
        output: port::Output<Doubler>,
    ) -> Result<BrokerFlow, Error>;
}

#[derive(Broker)]
#[broker(plan = Plan, error = Error)]
struct Broker {
    #[agent(thread)]
    doubler: agent::Cell<Doubler>,
}

impl Broker {
    fn handle_doubler(
        &mut self,
        plan: &mut dyn Plan,
        output: port::Output<Doubler>,
    ) -> Result<BrokerFlow, Error> {
        plan.handle_doubler(self, output)
    }
}

#[agentwire::test]
async fn test_thread() {
    struct TestPlan {
        result: Option<u32>,
    }
    impl Plan for TestPlan {
        fn handle_doubler(
            &mut self,
            _broker: &mut Broker,
            output: port::Output<Doubler>,
        ) -> Result<BrokerFlow, Error> {
            self.result = Some(output.value);
            Ok(BrokerFlow::Break)
        }
    }

    let mut broker = new_broker!();
    let mut plan = TestPlan { result: None };
    broker.enable_doubler().unwrap();

    let fence = Instant::now();
    broker.doubler.enabled().unwrap().send(port::Input::new(3)).await.unwrap();
    broker.run_with_fence(&mut plan, fence).await.unwrap();

    broker.disable_doubler();
    assert_eq!(plan.result, Some(6));
}
