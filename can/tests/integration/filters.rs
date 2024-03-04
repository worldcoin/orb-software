use can_rs::{
    filter::Filter, stream::FrameStream, Error, Id, CAN_DATA_LEN, CAN_RAW_FILTER_MAX,
};

use crate::can_address;

#[test]
#[ignore = "needs vcan interface"]
/// 9 filters in the array allows us to test getting filters by creating a 16-element array
/// and truncating it to 9 elements, see [`can_rs::socket::filters()`]
fn set_and_get_filters() {
    let stream = FrameStream::<CAN_DATA_LEN>::new(can_address()).unwrap();

    let mut filters = vec![
        Filter {
            id: Id::Standard(255),
            mask: 0,
        };
        9 // see doc above
    ];
    filters.sort_unstable();

    stream.set_filters(&filters).unwrap();
    assert_eq!(
        filters.first().unwrap(),
        stream.filters().unwrap().first().unwrap()
    );

    let mut recv_filters = stream.filters().unwrap();
    recv_filters.sort_unstable();

    assert!(!recv_filters.is_empty());
    assert_eq!(filters.len(), recv_filters.len());
    assert!(filters
        .iter()
        .zip(&recv_filters)
        .all(|(set, recv)| set == recv));
}

#[test]
#[ignore = "needs vcan interface"]
fn set_maximum_number_of_filters_successfully() {
    let stream = FrameStream::<CAN_DATA_LEN>::new(can_address()).unwrap();

    let filters = vec![
        Filter {
            id: Id::Standard(128),
            mask: 0,
        };
        CAN_RAW_FILTER_MAX
    ];

    stream.set_filters(&filters).unwrap();

    let recv_filters = stream.filters().unwrap();

    assert!(!recv_filters.is_empty());
    assert_eq!(filters.len(), recv_filters.len());
    assert!(filters
        .iter()
        .zip(&recv_filters)
        .all(|(set, recv)| set == recv));
}

#[test]
#[ignore = "needs vcan interface"]
fn set_maximum_number_of_filters_and_fail() {
    let stream = FrameStream::<CAN_DATA_LEN>::new(can_address()).unwrap();

    let filters = vec![
        Filter {
            id: Id::Standard(128),
            mask: 0,
        };
        CAN_RAW_FILTER_MAX + 1
    ];

    match stream.set_filters(&filters) {
        Ok(_) => panic!("should be impossible to set `CAN_RAW_FILTER_MAX + 1` filters"),
        Err(err) => match err {
            Error::CanFilterOverflow(_) => {}
            err => panic!("expected `Error::CANFilterOverflow`, instead found {}", err),
        },
    }
}
