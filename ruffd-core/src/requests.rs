use ruffd_types::Request;
use std::collections::HashMap;

lazy_static! {
    pub(crate) static ref REQUEST_REGISTRY: HashMap<&'static str, Request> = {
        let pairs = vec![];
        pairs
            .into_iter()
            .collect::<HashMap<&'static str, Request>>()
    };
}
