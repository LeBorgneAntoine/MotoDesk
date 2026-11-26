//! Typed access to Motodesk's data service, for the native interface.
//!
//! The interface never touches the database: it asks the local API. The API is called in the
//! same process ([`Local`]): a request is a function call into the service's handlers, with
//! their JSON, and no socket in between.
//!
//! One module per area of the API: the types the server answers with, and one function per
//! endpoint taking a [`Client`].

pub mod accounting;
pub mod assistant;
pub mod auth;
pub mod background;
pub mod backup;
pub mod clients;
pub mod cloud;
pub mod company;
pub mod dashboard;
pub mod data;
mod device_store;
pub mod employees;
pub mod error;
pub mod fiscal;
pub mod import;
pub mod local_account;
pub mod network;
pub mod network_accounts;
pub mod notices;
pub mod pos;
pub mod products;
pub mod recovery;
pub mod sales;
pub mod session;
pub mod settings;
pub mod shared;
pub mod stock;
pub mod sync;
pub mod templates;
pub mod token_store;
pub mod transport;
pub mod types;
pub mod updates;
pub mod workshop;

pub use cloud::Cloud;
pub use error::{ApiError, ApiResult};
pub use session::{Client, SessionEvent};
pub use token_store::TokenStore;
pub use transport::Local;
pub use types::{PublicUser, TokenBundle};
