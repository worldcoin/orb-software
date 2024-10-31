use agentwire::{
    agent::{self, Process as _},
    port::{self, Port, SharedPort},
    Agent, Broker, BrokerFlow,
};
use futures::prelude::*;
use rkyv::{Archive, Deserialize, Serialize};
use std::{mem::size_of, time::Instant};
use thiserror::Error;

#[derive(Clone, Default, Archive, Serialize, Deserialize, Debug)]
struct Doubler;

impl Port for Doubler {
    type Input = u32;
    type Output = u32;

    const INPUT_CAPACITY: usize = 0;
    const OUTPUT_CAPACITY: usize = 0;
}

impl SharedPort for Doubler {
    const SERIALIZED_INIT_SIZE: usize =
        size_of::<usize>() + size_of::<<Doubler as Archive>::Archived>();
    const SERIALIZED_INPUT_SIZE: usize =
        size_of::<usize>() + size_of::<<u32 as Archive>::Archived>();
    const SERIALIZED_OUTPUT_SIZE: usize =
        size_of::<usize>() + size_of::<<u32 as Archive>::Archived>();
}

impl Agent for Doubler {
    const NAME: &'static str = "doubler";
}

#[derive(Error, Debug)]
pub enum DoublerError {}

impl agent::Process for Doubler {
    type Error = DoublerError;

    fn run(self, mut port: port::RemoteInner<Self>) -> Result<(), Self::Error> {
        loop {
            let input = port.recv();
            let output = input.chain(input.value * 2);
            port.send(&output);
        }
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
    #[agent(process)]
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

fn init() {
    agent::process::init(|name, fd| match name {
        "doubler" => Ok(Doubler::call(fd)?),
        _ => panic!("unregistered agent {name}"),
    });
}

#[agentwire::test(init = init)]
async fn test_process() {
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
