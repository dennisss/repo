#![feature(proc_macro_hygiene, decl_macro, type_alias_enum_variants, generators)]

#[macro_use] extern crate serde_derive;
#[macro_use] extern crate error_chain;

extern crate futures_await as futures;

extern crate rand;
extern crate serde;
extern crate rmp_serde as rmps;
extern crate hyper;
extern crate tokio;
extern crate clap;
extern crate bytes;
extern crate raft;
extern crate core;


mod redis;
mod key_value;

use raft::errors::*;
use raft::server::{Server, ServerInitialState};
use raft::rpc::{Client, marshal, unmarshal};
use raft::node::*;
use std::path::Path;
use clap::{Arg, App};
use std::sync::{Arc, Mutex};
use futures::future::*;
use core::DirLock;
use rand::prelude::*;
use futures::prelude::*;
use futures::prelude::await;

use redis::resp::*;
use key_value::*;

/*
	Benchmark using:
	-	 redis-benchmark -t set,get -n 100000 -q -p 12345

	- In order to beat the 'set' benchmark, we must demonstrate efficient pipelining of all the concurrent requests to append an entry
		- 
*/

/*
	Some form of client interface is needed so that we can forward arbitrary entries to any server

*/

// XXX: See https://github.com/etcd-io/etcd/blob/fa92397e182286125c72bf52d95f9f496f733bdf/raft/raft.go#L113 for more useful config parameters


/*
	In order to make a server, we must at least have a server id 
	- First and for-most, if there already exists a file on disk with metadata, then we should use that
	- Otherwise, we must just block until we have a machine id by some other method
		- If an existing cluster exists, then we will ask it to make a new cluster id
		- Otherwise, the main() script must wait for someone to bootstrap us and give ourselves id 1
*/


/*
	Other scenarios
	- Server startup
		- Server always starts completely idle and in a mode that would reject external requests
		- If we have configuration on disk already, then we can use that
		- If we start with a join cli flag, then we can:
			- Ask the cluster to create a new unique machine id (we could trivially use an empty log entry and commit that to create a new id) <- Must make sure this does not conflict with the master's id if we make many servers before writing other data
	
		- If we are sent a one-time init packet via http post, then we will start a new cluster on ourselves
*/

/*
	Summary of event variables:
	- OnCommited
		- Ideally this would be a channel tht can pass the Arc references to the listeners so that maybe we don't need to relock in order to take things out of the log
		- ^ This will be consumed by clients waiting on proposals to be written and by the state machine thread waiting for the state machine to get fully applied 
	- OnApplied
		- Waiting for when a change is applied to the state machine
	- OnWritten
		- Waiting for when a set of log entries have been persisted to the log file
	- OnStateChange
		- Mainly to wake up the cycling thread so that it can 
		- ^ This will always only have a single consumer so this may always be held as light weight as possibl


	TODO: Future optimization would be to also save the metadata into the log file so that we are only ever writing to one append-only file all the time
		- I think this is how etcd implements it as well
*/


use raft::rpc::ServerService;
use raft::rpc::*;

struct RaftRedisServer {
	node: Arc<Node<KeyValueReturn>>,
	state_machine: Arc<MemoryKVStateMachine>
}


use redis::server::CommandResponse;
use redis::resp::RESPString;

impl redis::server::Service for RaftRedisServer {

	fn get(&self, key: RESPString) -> CommandResponse {
		let state_machine = &self.state_machine;

		let val = state_machine.get(key.as_ref());

		Box::new(ok(match val {
			Some(v) => RESPObject::BulkString(v), // NOTE: THis implies that we have no efficient way to serialize from references anyway
			None => RESPObject::Nil
		}))
	}

	// TODO: What is the best thing to do on errors?
	fn set(&self, key: RESPString, value: RESPString) -> CommandResponse {
		let state_machine = &self.state_machine;
		let node = &self.node;

		let op = KeyValueOperation::Set {
			key: key.as_ref().to_vec(),
			value: value.as_ref().to_vec(),
			expires: None,
			compare: None
		};

		// XXX: If they are owned, it is better to 
		let op_data = marshal(op).unwrap();

		Box::new(node.server.execute(op_data)
		.map_err(|e| {
			eprintln!("SET failed with {:?}", e);
			Error::from("Failed")
		})
		.map(|res| {
			RESPObject::SimpleString(b"OK"[..].into())
		}))

		/*
		Box::new(server.propose(raft::protos::ProposeRequest {
			data: LogEntryData::Command(op_data),
			wait: true
		})
		.map(|_| {
			RESPObject::SimpleString(b"OK"[..].into())
		}))
		*/
	}

	fn del(&self, key: RESPString) -> CommandResponse {
		// TODO: This requires knowledge of how many keys were actually deleted (for the case of non-existent keys)

		let state_machine = &self.state_machine;
		let node = &self.node;

		let op = KeyValueOperation::Delete {
			key: key.as_ref().to_vec()
		};

		// XXX: If they are owned, it is better to 
		let op_data = marshal(op).unwrap();

		Box::new(node.server.execute(op_data)
		.map_err(|e| {
			eprintln!("DEL failed with {:?}", e);
			Error::from("Failed")
		})
		.map(|res| {
			RESPObject::Integer(if res.success { 1 } else { 0 })
		}))
		
		/*
		Box::new(server.propose(raft::protos::ProposeRequest {
			data: LogEntryData::Command(op_data),
			wait: true
		})
		.map(|_| {
			RESPObject::Integer(1)
		}))*/
	}

	fn publish(&self, channel: RESPString, object: RESPObject) -> Box<Future<Item=usize, Error=Error> + Send> {
		Box::new(ok(0))
	}

	fn subscribe(&self, channel: RESPString) -> Box<Future<Item=(), Error=Error> + Send> {
		Box::new(ok(()))
	}

	fn unsubscribe(&self, channel: RESPString) -> Box<Future<Item=(), Error=Error> + Send> {
		Box::new(ok(()))
	}
}

/*

	XXX: DiscoveryService will end up requesting ourselves in the case of starting up the services themselves starting up
	-> Should be ideally topology agnostic
	-> We only NEED to do a discovery if we are not 

	-> We always want to have a discovery service
		-> 


	-> Every single server if given a seed list should try to reach that seed list on startup just to try and get itself in the cluster
		-> Naturally in the case of a bootstrap

	-> In most cases, if 

*/

#[async]
fn main_task() -> Result<()> {
	let matches = App::new("Raft")
		.about("Sample consensus reaching node")
		.arg(Arg::with_name("dir")
			.long("dir")
			.short("d")
			.value_name("DIRECTORY_PATH")
			.help("An existing directory to store data file for this unique instance")
			.required(true)
			.takes_value(true))
		// TODO: Also support specifying our rpc listening port
		.arg(Arg::with_name("join")
			.long("join")
			.short("j")
			.value_name("SERVER_ADDRESS")
			.help("Address of a running server to be used for joining its cluster if this instance has not been initialized yet")
			.takes_value(true))
		.arg(Arg::with_name("bootstrap")
			.long("bootstrap")
			.help("Indicates that this should be created as the first node in the cluster"))
		.get_matches();


	// TODO: For now, we will assume that bootstrapping is well known up front although eventually to enforce that it only ever occurs exactly once, we may want to have an admin externally fire exactly one request to trigger it
	// But even if we do pass in bootstrap as an argument, it is still guranteed to bootstrap only once on this machine as we will persistent the bootstrapped configuration before talking to other servers in the cluster

	let dir = Path::new(matches.value_of("dir").unwrap()).to_owned();
	let bootstrap = matches.is_present("bootstrap");
	let seed_list: Vec<String> = vec![
		"http://127.0.0.1:4001".into(),
		"http://127.0.0.1:4002".into()
	];


	// XXX: Need to store this somewhere more persistent so that we don't lose it
	let lock = DirLock::open(&dir)?;
	
	// XXX: Right here if we are able to retrieve a snapshot, then we are allowed to do that 
	// But we will end up thinking of all the stuff initially on disk as one atomic unit that is initially loaded
	let state_machine = Arc::new(MemoryKVStateMachine::new());
	let last_applied = 0;

	let node = await!(Node::start(NodeConfig {
		dir: lock,
		bootstrap,
		seed_list,
		state_machine: state_machine.clone(),
		last_applied
	}))?;

	let client_server = Arc::new(redis::server::Server::new(RaftRedisServer {
		node: node.clone(), state_machine: state_machine.clone()
	}));

	let client_task = redis::server::Server::start(client_server.clone(), (5000 + node.id) as u16);

	await!(client_task);

	Ok(())
}


fn main() -> Result<()> {

	tokio::run(lazy(|| {
		main_task()
		.map_err(|e| {
			eprintln!("{:?}", e);
			()
		})
	}));

	// This is where we would perform anything needed to manage regular client requests (and utilize the server handle to perform operations)
	// Noteably we want to respond to clients with nice responses telling them specifically if we are not the actual leader and can't actually fulfill their requests

	Ok(())
}

