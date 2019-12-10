use reqwest::Response;
use crate::clients::directory::presence::Topology;

use serde::Deserialize;


pub struct Request {
    base_url: String,
}

pub trait PresenceTopologyGetRequester {
    fn new(base_url: String) -> Self;
    fn get(&self) -> Result<Topology, reqwest::Error>;
}

impl PresenceTopologyGetRequester for Request {
    fn new(base_url: String) -> Self {
        Request { base_url }
    }

    fn get(&self) -> Result<Topology, reqwest::Error> {
        let url = format!("{}/topology", self.base_url);
        reqwest::get(&url)?.json()?
    }
}

mod healthcheck_requests {
    use super::*;

    #[cfg(test)]
    use mockito::mock;

    #[cfg(test)]
    mod on_a_400_status {
        use super::*;

        #[test]
        #[should_panic]
        fn it_returns_an_error() {
            let _m = mock("GET", "/healthcheck").with_status(400).create();
            let req = Request::new(mockito::server_url());
            assert_eq!(true, req.get().is_err());
        }
    }

    #[cfg(test)]
    mod on_a_200 {
        use super::*;

        #[test]
        fn it_returns_a_response_with_200_status() {
            let _m = mock("GET", "/healthcheck").with_status(200).create();
            let req = Request::new(mockito::server_url());

            assert_eq!(true, req.get().is_ok());
        }
    }
}
