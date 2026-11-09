//! `frontdoor`: the front door chat. The router is code, not a model (FD-1); the abilities
//! index (`crate::abilities`) holds the request kinds it routes by.

pub mod adapters;
pub mod implicit_log;
pub mod labeller;
pub mod packet;
pub mod rating_log;
pub mod router;
pub mod sticky;
pub mod thread;
