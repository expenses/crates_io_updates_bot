use matrix_bot_api::{
	MatrixBot, MessageType, ActiveBot,
	handlers::{HandleResult, stateless_handler::StatelessHandler}
};
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::Mutex;
use std::thread::{spawn, sleep};
use std::time::Duration;
use structopt::StructOpt;

lazy_static! {
	static ref VERSION_MAP: Mutex<HashMap<String, String>> = Mutex::new(HashMap::new());
	static ref CRATES_IO_CLIENT: crates_io_api::SyncClient = crates_io_api::SyncClient::new();
	static ref OPTS: Options = Options::from_args();
}

const HELP: &str = "
!add <crate>... - Add crates to be watched;
!list - List watched crates.
!remove <crate>... - Remove crates from watch list.
!help - Show this dialog.
";

fn latest_version(crate_name: &str) -> Result<String, crates_io_api::Error> {
	CRATES_IO_CLIENT.get_crate(crate_name)
		.map(|info| info.versions[0].num.clone())
}

#[derive(StructOpt)]
struct Options {
	#[structopt(short, long)]
	username: String,
	#[structopt(short, long)]
	password: String,
	#[structopt(short, long)]
	room: String,
	#[structopt(short, long, default_value = "https://matrix-client.matrix.org")]
	homeserver_url: url::Url,
	#[structopt(short, long, help = "Don't print verbosely")]
	quiet: bool,
	#[structopt(short = "f", long, default_value = "600", help = "How often to check crates.io")]
	update_frequency: u64
}

fn register_handles() -> StatelessHandler {
	let mut handler = StatelessHandler::new();

	handler.register_handle("list", |bot, msg, _| {
		if msg.room != OPTS.room {
			return HandleResult::StopHandling;
		}

		let output = {
			let map = VERSION_MAP.lock().unwrap();

			if map.is_empty() {
				"No crates being watched".to_string()
			} else {
				map.iter()
					.map(|(crate_name, version)| format!("`{}`:\t`{}`\n", crate_name, version))
					.collect()
			}
		};

		bot.send_message(&output, &OPTS.room, MessageType::TextMessage);
		HandleResult::StopHandling
	});

	handler.register_handle("add", |bot, msg, tail| {
		if msg.room != OPTS.room {
			return HandleResult::StopHandling;
		}

		let mut output: String = tail.split(' ')
			.filter(|crate_name| !crate_name.is_empty())
			.map(|crate_name| {
				match latest_version(crate_name) {
					Ok(latest) => {
						VERSION_MAP.lock().unwrap().insert(crate_name.to_string(), latest.clone());
						format!("Added `{}` version `{}`\n", crate_name, latest)
					},
					Err(error) => match error {
						crates_io_api::Error::NotFound => format!("Error: `{}` not found\n", crate_name),
						error @ _ => format!("Error: `{}`, {}\n", crate_name, error)
					}
				}
			})
			.collect();

		if output.is_empty() {
			output += "No crates being watched";
		}

		bot.send_message(&output, &OPTS.room, MessageType::TextMessage);
		HandleResult::StopHandling
	});

	handler.register_handle("remove", |bot, msg, tail| {
		if msg.room != OPTS.room {
			return HandleResult::StopHandling;
		}

		let output: String = tail.split(' ')
			.filter(|crate_name| !crate_name.is_empty())
			.map(|crate_name| {
				match VERSION_MAP.lock().unwrap().remove(crate_name) {
					Some(version) => format!("Removed `{}` (version `{}`)\n", crate_name, version),
					None => format!("Error: `{}` being watched\n", crate_name)
				}
			})
			.collect();

		bot.send_message(&output, &OPTS.room, MessageType::TextMessage);
		HandleResult::StopHandling
	});

	handler.register_handle("help", |bot, msg, _| {
		if msg.room != OPTS.room {
			return HandleResult::StopHandling;
		}

		bot.send_message(HELP, &OPTS.room, MessageType::TextMessage);
		HandleResult::StopHandling
	});

	handler
}

fn update_check_loop(update_bot: ActiveBot) {
	loop {
		sleep(Duration::from_secs(OPTS.update_frequency));

		let output: String = VERSION_MAP.lock().unwrap().iter_mut()
			.map(|(crate_name, version)| {
				let latest = latest_version(&crate_name).unwrap();

				if *version != latest {
					let output = format!("`{}` updated from version `{}` to `{}`!", crate_name, version, latest);
					*version = latest;
					output
				} else {
					String::new()
				}
			})
			.collect();

		if !output.is_empty() {
			update_bot.send_message(&output, &OPTS.room, MessageType::TextMessage);
		}
	}
}

fn main() {
	let mut bot = MatrixBot::new(register_handles());

	bot.set_verbose(!OPTS.quiet);

	bot.login(&OPTS.username, &OPTS.password, &OPTS.homeserver_url.as_str());

	let mut active_bot = bot.active_bot();

	bot.init_handlers(&active_bot);

	let update_bot = active_bot.clone();

	let handle = spawn(move || update_check_loop(update_bot));

	loop {
		if !bot.recv_and_handle(&mut active_bot) {
			break;
		}
	}

	handle.join().unwrap();
}
