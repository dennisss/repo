#![feature(proc_macro_hygiene, decl_macro, type_alias_enum_variants, generators)]

#[macro_use] extern crate serde_derive;
#[macro_use] extern crate error_chain;

extern crate futures_await as futures;
extern crate rand;
extern crate serde;
extern crate rmp_serde as rmps;
extern crate hyper;
extern crate tokio;
extern crate bytes;


pub mod errors {
	error_chain! {
		foreign_links {
			Io(::std::io::Error);
			HTTP(hyper::Error);
		}
	}
}

mod sync;
pub mod protos; // TODO: Eventually make this private again
pub mod rpc;
pub mod state_machine;

pub mod log; // XXX: Likewise should be private
mod state;
pub mod consensus;
