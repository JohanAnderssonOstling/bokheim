extern crate core;

pub mod db;
pub mod epub;
pub mod home;
pub mod library;
pub mod scanner;
mod sync;
mod transfer;

pub type BookID = u64;