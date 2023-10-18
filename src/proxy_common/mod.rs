use std::error::Error;
use env_logger;
use log::LevelFilter;
use std::env;

/*******************
 * IMPLEMENT ERROR *
 *******************/

 #[derive(Debug)]
 pub(crate) struct ProxyErr
 {
	 message : String,
 }
 
 impl Error for ProxyErr {}
 
 impl ProxyErr {
	 // Create a constructor method for your custom error
	 pub(crate) fn new(message: &str) -> ProxyErr {
		ProxyErr {
				message: message.to_string(),
		}
 
	 }
 }
 
 impl std::fmt::Display for ProxyErr {
	 fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		  write!(f, "{}", self.message)
	 }
 }

 pub fn init_log()
 {
	let env = env_logger::Env::new()
							.default_filter_or("info");
	env_logger::init_from_env(env);
 }