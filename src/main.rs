extern crate async_std;
use async_std::prelude::*;

/// error handling-related utilities
mod anyhow {
  pub type Result<T, E = Error> = std::result::Result<T, E>;
  pub type Error = std::boxed::Box<dyn std::error::Error + 'static + Send + Sync>;
}

/// simple console logging utilities
#[macro_use]
#[allow(unused_macros)]
mod log {
  // https://cdn.discordapp.com/attachments/542264318465671170/718499342176223305/unknown.png
  // https://cdn.discordapp.com/attachments/542264318465671170/710063198111399936/unknown.png
  macro_rules! log {
    (target $target:expr, $fmt:expr) => {
      println!("\x1b[2m[log] \x1b[1;34m{}\x1b[0m\x1b[1m:\x1b[0m {}", $target, $fmt);
    };
    (target $target:expr, $fmt:expr, $($args:expr),+) => {
      println!("\x1b[2m[log] \x1b[1;34m{}\x1b[0m\x1b[1m:\x1b[0m {}", $target, format_args!($fmt, $($args),+));
    };
  }
  macro_rules! wrn {
    (target $target:expr, $fmt:expr) => {
      println!(
        "\x1b[1;33m[wrn] \x1b[1;34m{}\x1b[0m\x1b[1m:\x1b[0m {}",
        $target, $fmt
      );
    };
    (target $target:expr, $fmt:expr, $args:expr) => {
      println!(
        "\x1b[1;33m[wrn] \x1b[1;34m{}\x1b[0m\x1b[1m:\x1b[0m {}",
        $target,
        format_args!($fmt, $args)
      );
    };
  }
}

mod irc {
  pub mod twitch {
    #[derive(Debug)]
    pub enum Message {
      Ping,
      Dummy(String),
      // prefix, channel, message
      PrivMsg(String, String, String),
    }
  }
}

// read thread - irc twitch message -> parse thread - effect -> send thread
#[derive(Debug)]
enum Effect {
  // target (user or channel), text
  Say(String, String),
}

#[derive(Debug)]
pub struct Config {
  oauth: String,
  nickname: String,
  channels: Vec<String>,
}

async fn receive_something(
  stream_writer: &async_std::sync::Arc<
    async_std::sync::Mutex<async_std::io::BufWriter<async_std::net::TcpStream>>,
  >,
  rx: async_std::sync::Receiver<irc::twitch::Message>,
  config: &Config,
) -> anyhow::Result<()> {
  while let Ok(m) = rx.recv().await {
    println!("received {:?}", m);
    if let irc::twitch::Message::Ping = m {
      stream_writer
        .lock()
        .await
        .write(b"PONG :tmi.twitch.tv\r\n")
        .await?;
      println!("sent pong");
    } else if let irc::twitch::Message::PrivMsg(prefix, channel, text) = m {
      if text.starts_with('%') {
        if None == text.chars().nth(1) {
          // invalid command
          continue;
        }
        let text = text.get(1..).unwrap();
        if let Some(i) = text.find(' ') {
          // parse command with arguments
          let (cmd, args) = text.split_at(i);
          match cmd {
            "say" => {
              stream_writer
                .lock()
                .await
                .write(format!("PRIVMSG {} :{}\r\n", channel, args).as_bytes())
                .await?;
              eprintln!("sent 'say' command to {}, containing '{}'", channel, args);
            }
            _ => {}
          }
        } else {
          // parse simple command
          match text {
            "why" => {
              stream_writer
                .lock()
                .await
                .write(format!("PRIVMSG {} :why not?\r\n", channel).as_bytes())
                .await?;
            }
            "dump" => {
              stream_writer
                .lock()
                .await
                .write(
                  format!(
                    "PRIVMSG {} :core dumped, contact system administrator for help\r\n",
                    channel
                  )
                  .as_bytes(),
                )
                .await?;
            }
            "help" => {
              stream_writer
                .lock()
                .await
                .write(
                  format!("PRIVMSG {} :help available under /dev/urandom\r\n", channel).as_bytes(),
                )
                .await?;
            }
            _ => {}
          }

          eprintln!("sent response to {}", channel);
        }

        stream_writer.lock().await.flush().await?;
        eprintln!("flushed messages");
        continue;
      }

      if text
        .split_ascii_whitespace()
        .any(|s| s.contains(&config.nickname))
      {
        if let Some(i) = prefix.find('!') {
          let nickname = prefix.split_at(i).0.get(1..).unwrap();
          stream_writer
            .lock()
            .await
            .write(format!("PRIVMSG {} :nyanpasu, {}\r\n", channel, nickname).as_bytes())
            .await?;
          eprintln!("sent hello-message to {}, {}", channel, nickname);
        }
      }
    }

    stream_writer.lock().await.flush().await?;
  }

  return Ok(());
}

mod console {
  use async_std::prelude::*;

  pub async fn init(
    stream_writer: &async_std::sync::Arc<
      async_std::sync::Mutex<async_std::io::BufWriter<async_std::net::TcpStream>>,
    >,
  ) -> crate::anyhow::Result<()> {
    let console_listener = async_std::net::TcpListener::bind("127.0.0.1:9000").await?;
    let mut incoming = console_listener.incoming();
    while let Some(_stream) = incoming.next().await {
      let stream = _stream?;
      let (mut reader, mut writer) = (
        async_std::io::BufReader::new(stream.clone()),
        async_std::io::BufWriter::new(stream),
      );
      let mut running = true;
      let mut current_twitch_channel = String::new();
      while running {
        writer
          .write(
            if current_twitch_channel.is_empty() {
              "(shikura) :: ".into()
            } else {
              format!("(shikura) [{}] :: ", &current_twitch_channel)
            }
            .as_bytes(),
          )
          .await?;
        writer.flush().await?;
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        //eprintln!("[console] {:?}", line);
        let line = line.trim_end();
        if let Some(i) = line.find(' ') {
          let (cmd, args) = line.split_at(i);
          let args = args.trim_start();
          match cmd {
            "say" => {
              let (channel, text) = if !current_twitch_channel.is_empty() {
                (current_twitch_channel.as_str(), args)
              } else {
                args.split_at(args.find(' ').unwrap())
              };
              let text = text.trim_start();
              stream_writer
                .lock()
                .await
                .write(format!("PRIVMSG {} :{}\r\n", channel, text).as_bytes())
                .await?;
              log!(target "console", "sent '{}' to {}", text, channel);
            }
            "join" => {
              current_twitch_channel = args.into();
              log!(target "console", "switched to {}", &current_twitch_channel);
            }
            _ => {}
          }
        } else {
          match line {
            "quit" => running = false,
            "part" => current_twitch_channel = "".into(),
            _ => {}
          }
        }

        stream_writer.lock().await.flush().await?;
      }
    }
    return Ok(());
  }
}

mod twitch {
  use async_std::prelude::*;

  pub struct State {
    pub pair: (
      async_std::io::BufReader<async_std::net::TcpStream>,
      async_std::io::BufWriter<async_std::net::TcpStream>,
    ),
  }

  pub async fn connect(bot_config: &crate::Config) -> crate::anyhow::Result<State> {
    let stream = async_std::net::TcpStream::connect("irc.chat.twitch.tv:6667").await?;
    let (stream_reader, mut stream_writer) = (
      async_std::io::BufReader::new(stream.clone()),
      async_std::io::BufWriter::new(stream),
    );

    stream_writer
      .write(format!("PASS {}\r\n", bot_config.oauth).as_bytes())
      .await?;
    stream_writer
      .write(format!("NICK {}\r\n", bot_config.nickname).as_bytes())
      .await?;
    stream_writer.flush().await?;

    log!(target "twitch", "connecting..");

    for channel in &bot_config.channels {
      stream_writer
        .write(format!("JOIN {}\r\n", channel).as_bytes())
        .await?;
      log!(target "twitch", "joined {}", channel);
    }
    stream_writer.flush().await?;

    return Ok(State {
      pair: (stream_reader, stream_writer),
    });
  }
}

async fn twitch_receive_something(
  stream_reader: &async_std::sync::Arc<
    async_std::sync::Mutex<async_std::io::BufReader<async_std::net::TcpStream>>,
  >,
  tx: async_std::sync::Sender<irc::twitch::Message>,
) -> anyhow::Result<()> {
  let stream_reader_for_twitch_task = stream_reader;
  let mut line = String::new();

  while let Ok(_) = stream_reader_for_twitch_task
    .lock()
    .await
    .read_line(&mut line)
    .await
  {
    let _line = line.trim_end();
    // TODO: parse irc messages properly
    if _line.starts_with("PING") {
      tx.send(irc::twitch::Message::Ping).await;
      continue;
    }
    if _line.starts_with(':') {
      if let Some(i) = _line.find(' ') {
        let (prefix, rest) = _line.split_at(i);
        let rest = rest.trim_start();
        if let Some(j) = rest.find(' ') {
          let (cmd, rest_) = rest.split_at(j);
          let rest_ = rest_.trim_start();
          if cmd == "PRIVMSG" {
            let (channel, text) = rest_.split_at(rest_.find(' ').unwrap());
            let text = text.get(2..).unwrap();
            eprintln!(
              "prefix = {:?}, channel = {:?}, text = {:?}",
              prefix, channel, text
            );
            tx.send(irc::twitch::Message::PrivMsg(
              prefix.into(),
              channel.into(),
              text.into(),
            ))
            .await;
          }
        } else {
          // cmd is just rest
        }
      }
    } else {
      tx.send(irc::twitch::Message::Dummy(_line.into())).await;
    }
    println!("{}", _line);
    line.clear();
  }

  return Ok(());
}

fn main() -> anyhow::Result<()> {
  let mut iargs = std::env::args();
  let binary_name = iargs.next().unwrap();

  let config_file_path = match iargs.next() {
    Some(a) => a,
    None => {
      eprintln!("Usage: {} <config> [<log>]", binary_name);
      // missing argument is not an error!
      return Ok(());
    }
  };

  let log_file_path = iargs.next().unwrap_or("log".into());

  let config_file = std::fs::File::open(config_file_path)?;
  let config_file = std::io::BufReader::new(config_file);
  use std::io::BufRead;
  let mut config_file_lines = config_file.lines();

  let oauth_token = config_file_lines.next().unwrap()?;
  let nickname = config_file_lines.next().unwrap()?;
  let channels: Vec<String> = config_file_lines.map(|a| a.unwrap()).collect();

  let bot_config = Config {
    nickname,
    channels,
    oauth: oauth_token,
  };

  // start async context
  async_std::task::block_on(async {
    log!(target "shikura", "starting..");

    let twitch_state = twitch::connect(&bot_config).await?;

    let log_file = async_std::fs::OpenOptions::new()
      .append(true)
      .create(true)
      .open(log_file_path)
      .await?;
    let _log_file_writer = async_std::io::BufWriter::new(log_file);

    let stream_reader = async_std::sync::Arc::new(async_std::sync::Mutex::new(twitch_state.pair.0));
    let stream_writer = async_std::sync::Arc::new(async_std::sync::Mutex::new(twitch_state.pair.1));

    let (tx, rx) = async_std::sync::channel::<irc::twitch::Message>(1);

    let stream_writer_for_console_task = stream_writer.clone();
    let console_task = async_std::task::spawn(async move {
      eprintln!("{:?}", console::init(&stream_writer_for_console_task).await);
    });

    let stream_reader_for_twitch_task = stream_reader.clone();
    let tx_for_twitch_task = tx.clone();
    let twitch_task = async_std::task::spawn(async move {
      eprintln!(
        "{:?}",
        twitch_receive_something(&stream_reader_for_twitch_task, tx_for_twitch_task).await
      );
    });

    let stream_writer_for_receive_task = stream_writer.clone();
    let receive_task = async_std::task::spawn(async move {
      eprintln!(
        "{:?}",
        receive_something(&stream_writer_for_receive_task, rx, &bot_config).await
      );
    });

    receive_task.await;
    twitch_task.await;
    console_task.await;

    return anyhow::Result::<()>::Ok(());
  })?;

  return Ok(());
}
