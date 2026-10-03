use orb_contract_schema::{contract, gen_schemas};

#[contract]
struct Example;

#[test]
fn registers_contract_and_generates_schema() {
    let name = concat!(module_path!(), "::Example");
    let contract = gen_schemas()
        .find(|contract| contract.name == name)
        .expect("Example contract should be registered");

    let _schema = (contract.schema)();
}
