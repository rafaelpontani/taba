//! What the AWS protocols (SQS and SNS) have in common: who is calling (the signature and the credentials), the errors, the
//! HTTP answer, the XML and the Query form. Nothing here knows an action of a service (doc 39).

pub mod auth;
pub mod credentials;
pub mod errors;
pub mod form;
pub mod params;
pub mod reply;
pub mod sigv4;
pub mod xml;
