//! `ahjo`: the node, and the CLI over its commands.

use std::path::PathBuf;

use ahjo::{AREAS, install};
use ahjo_node_kit::identity::Identity;
use ahjo_node_kit::{Config, Options, cli, config, hook, listing, mcp};
use anyhow::{Context, Result, bail};

const USAGE: &str = "usage:
  ahjo serve [--config file] [--listen addr:port]
  ahjo mcp [--node addr:port]
  ahjo help [--node id|url]                            list a node's commands
  ahjo <noun> <verb> [args] [--json] [--node id|url]   the node's commands, e.g. ahjo session list
  ahjo hook <url> [--then command...] [payload]        (run by a program's own hook)
  ahjo kinds                                           who owns which frame kinds
  ahjo install [--root dir] [--unit name] [--listen addr:port] ...
                                                        this build as the systemd user unit
  ahjo upgrade [--from file] [--unit name] ...         replace the installed build; sessions stay
  ahjo version [--node addr:port]                      this build's version and the node's
  ahjo --version                                       this build's version";

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let args: Vec<String> = args.collect();
    match command.as_str() {
        hook::VERB => {
            hook::report(&args);
            Ok(())
        }
        "" | "-h" | "--help" | "help" => {
            let node = help_node(&args)?;
            println!("{USAGE}");
            match cli::overview(node).await {
                Ok(commands) => println!(
                    "\nCommands from the running node (ahjo <noun> <verb> --help):\n{commands}"
                ),
                Err(error) if node.is_none() => {
                    println!("\nThe node's commands show here while it runs: {error:#}")
                }
                Err(error) => return Err(error.context("listing the requested node's commands")),
            }
            Ok(())
        }
        "serve" => serve(&args).await,
        "install" | "upgrade" => std::process::exit(install::run(&command, &args).await),
        "--version" => {
            println!("ahjo {}", ahjo_node_kit::VERSION);
            Ok(())
        }
        "version" => version(&args).await,
        "kinds" => {
            print!("{}", listing::kinds_table(AREAS));
            Ok(())
        }
        "mcp" => {
            let node = match args.as_slice() {
                [] => ahjo_node_kit::address::local(),
                [flag, node] if flag == "--node" => node.clone(),
                _ => bail!("{USAGE}"),
            };
            mcp::stdio(&node).await
        }
        noun if noun.starts_with('-') => bail!("{USAGE}"),
        noun => {
            let code = cli::run(noun, &args, &mut std::io::stdout()).await?;
            std::process::exit(code);
        }
    }
}

/// The optional node for the top-level help overview.
fn help_node(args: &[String]) -> Result<Option<&str>> {
    match args {
        [] => Ok(None),
        [flag, node] if flag == "--node" && !node.is_empty() => Ok(Some(node)),
        [flag] if flag.starts_with("--node=") && flag.len() > "--node=".len() => {
            Ok(Some(&flag["--node=".len()..]))
        }
        _ => bail!("usage: ahjo help [--node id|url]"),
    }
}

/// This build's version, and the version of the node that answers.
async fn version(args: &[String]) -> Result<()> {
    let node = match args {
        [] => ahjo_node_kit::address::local(),
        [flag, node] if flag == "--node" => node.clone(),
        _ => bail!("{USAGE}"),
    };
    println!("ahjo {}", ahjo_node_kit::VERSION);
    let url = match node.contains("://") {
        true => node.clone(),
        false => format!("http://{node}"),
    };
    let client = ahjo_node_kit::NodeClient::new(Some(&format!("{}/", url.trim_end_matches('/'))))?;
    match client
        .request(axum::http::Method::GET, "/api/health", None, None)
        .await
    {
        Ok((health, _)) => println!(
            "node {} on {node} runs {}",
            health["node"].as_str().unwrap_or("?"),
            health["version"].as_str().unwrap_or("?")
        ),
        Err(error) => println!("no node answers on {node}: {error:#}"),
    }
    Ok(())
}

/// Serves until Ctrl-C or SIGTERM.
async fn serve(args: &[String]) -> Result<()> {
    let dir = Identity::config_dir()?;
    let mut file = dir.join(config::FILE);
    let mut listen = None;
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let mut value = || args.next().with_context(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--config" => file = PathBuf::from(value()?),
            "--listen" => listen = Some(value()?.clone()),
            _ => bail!("{USAGE}"),
        }
    }
    let config = Config::load(&file)?;
    let running = ahjo_node_kit::start(
        Options {
            dir,
            config,
            listen,
        },
        AREAS,
    )
    .await?;
    stopped().await;
    running.stop().await
}

async fn stopped() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}
