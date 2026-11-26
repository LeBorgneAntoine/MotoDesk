//! Typed access to Motodesk's data service, for the native interface.
//!
//! The interface never touches the database: it asks the local API, exactly as the Svelte
//! interface did over `http://127.0.0.1:17300`. Here the API is called in the same process
//! ([`Local`]), so a request is a function call with the same handlers and the same JSON, and
//! no socket in between.
//!
//! One module per area of the API, each a port of `desktop/src/lib/api/<area>.ts`:
//! the types the server answers with, and one function per endpoint taking a [`Client`].

pub mod accounting;
pub mod auth;
pub mod backup;
pub mod clients;
pub mod cloud;
pub mod company;
pub mod dashboard;
pub mod data;
pub mod employees;
pub mod error;
pub mod fiscal;
pub mod import;
pub mod local_account;
pub mod network;
pub mod network_accounts;
pub mod products;
pub mod recovery;
pub mod sales;
pub mod session;
pub mod settings;
pub mod shared;
pub mod stock;
pub mod sync;
pub mod token_store;
pub mod transport;
pub mod types;
pub mod workshop;

pub use cloud::Cloud;
pub use error::{ApiError, ApiResult};
pub use session::{Client, SessionEvent};
pub use token_store::TokenStore;
pub use transport::Local;
pub use types::{PublicUser, TokenBundle};
