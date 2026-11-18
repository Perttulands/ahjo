//! `ahjo`: the node, and the CLI over its commands.

use std::path::PathBuf;

use ahjo::{AREAS, install};
use ahjo_core::Error;
use ahjo_node_kit::identity::Identity;
use ahjo_node_kit::{
    Config, NodeClient, NodeError, Options, Unreachable, cli, config, hook, listing, mcp,
};
use anyhow::Result;

const USAGE: &str = "ahjo: the node of Ahjo Swarm, and the command line over its commands.

usage:
  ahjo <noun> <verb> [args] [--json] [--node id|url]   a command of the node, e.g. ahjo session list
  ahjo <noun> [--node id|url]                          a noun's verbs and their arguments
  ahjo help [noun|--all] [--node id|url]               the nouns; a noun's verbs; every command
  ahjo version [--json] [--node addr:port]             this build, and the node that answers
  ahjo serve [--config file] [--listen addr:port]      run the node here, in the foreground
  ahjo install [--app file] [--root dir] ...           this build as a systemd user unit
  ahjo upgrade [--app file] ...                        replace the installed build; sessions stay
  ahjo uninstall [--unit name]                         take the unit and its build away
  ahjo mcp [--area name]... [--node addr:port]         the node's commands as MCP tools on stdio
  ahjo kinds                                           who owns which frame kinds
  ahjo hook <url> [--then command...] [payload]        (run by a program's own hook)
  ahjo --version                                       this build's version

ahjo <command> --help says more. A <required> argument may also be given as --name value.
With --json the answer is JSON on stdout, and an error is {\"error\":{\"code\",\"message\"}} on stderr.
Exit codes: 0 done, 1 the command failed, 2 the command line was wrong, 3 no node answers.";

const SERVE_USAGE: &str = "usage: ahjo serve [--config file] [--listen addr:port]";
const MCP_USAGE: &str = "usage: ahjo mcp [--area name]... [--node addr:port]";
const VERSION_USAGE: &str = "usage: ahjo version [--json] [--node addr:port]";
const HELP_USAGE: &str = "usage: ahjo help [noun|--all] [--node id|url]";

/// What `ahjo <command> --help` says for the binary's own commands.
fn own_help(command: &str) -> Option<String> {
    Some(match command {
        "serve" => format!(
            "{SERVE_USAGE}
Run the node in the foreground until Ctrl-C or SIGTERM. Terminal sessions
belong to tmux and live on when it stops. ahjo install runs the same thing
as a systemd user unit.

  --config  file: the config to read (default {})
  --listen  addr:port: where to listen (default [node] listen in the config,
            or else this environment's port on 127.0.0.1)",
            Identity::config_dir()
                .map(|dir| dir.join(config::FILE).display().to_string())
                .unwrap_or_else(|_| "~/.config/ahjo/config.toml".into())
        ),
        "mcp" => format!(
            "{MCP_USAGE}
Serve the node's commands as MCP tools over stdin and stdout, for an agent
harness that starts its servers as programs. The tools are the commands
ahjo help lists, named <noun>_<verb>. Each area is a server of its own, so
a harness that needs only some pays only for their tools; without --area
it serves every one. Over HTTP the same are /mcp/<area>[,<area>...] and /mcp.

  --area  one of {} (node: the node's own commands); again for more
  --node  addr:port: the node to relay to (default: this machine's)",
            servers().join(", ")
        ),
        "version" => format!(
            "{VERSION_USAGE}
Print this build's version and the version of the node that answers. Exits 3
when no node answers. ahjo --version prints only this build's.

  --json  the same as JSON: version, and node (id, address, version) or null
  --node  addr:port: the node to ask (default: this machine's)"
        ),
        "help" => format!(
            "{HELP_USAGE}
List the node's nouns, each with its verbs; with a noun, its verbs and their
arguments; with --all, every command with what it does.

  --all   every command, one line each

  --node  id|url: the node whose commands to list (default: this machine's)"
        ),
        "kinds" => "usage: ahjo kinds
Print which area owns which frame kinds on the socket between the node and
the app. For someone changing Ahjo."
            .into(),
        hook::VERB => "usage: ahjo hook <url> [--then command...] [payload]
Post a payload (the last argument, or stdin) to a path on this machine's
node. Run by a harness's own hook, which ahjo sets up; it always exits 0 so
a hook never stalls the program that ran it. After --then comes another
command to run with the same payload."
            .into(),
        "install" => install::INSTALL_HELP.into(),
        "upgrade" => install::UPGRADE_HELP.into(),
        "uninstall" => install::UNINSTALL_HELP.into(),
        _ => return None,
    })
}

fn usage(why: impl std::fmt::Display, usage: &str) -> anyhow::Error {
    cli::Usage(format!("{why}\n{usage}")).into()
}

fn asks_help(args: &[String]) -> bool {
    args.iter().any(|a| a == "--help" || a == "-h")
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let args: Vec<String> = args.collect();
    let json = args.iter().any(|a| a == "--json");
    let code = match run(&command, &args).await {
        Ok(code) => code,
        Err(error) => failed(&error, json),
    };
    std::process::exit(code);
}

/// Says why a command did not run, on stderr, and answers its exit code: 3
/// when no node answers, 2 when the command line was wrong, else 1.
fn failed(error: &anyhow::Error, json: bool) -> i32 {
    let (shaped, exit) = if let Some(unreachable) = error.downcast_ref::<Unreachable>() {
        let message = format!(
            "{unreachable}. Start it with ahjo serve, or with systemctl --user start ahjo \
             where ahjo install put it; --node names another node"
        );
        (Error::new("no_node", message), 3)
    } else if let Some(wrong) = error.downcast_ref::<cli::Usage>() {
        (Error::new("bad_arguments", wrong.to_string()), 2)
    } else if let Some(known) = error.downcast_ref::<Error>() {
        (known.clone(), 1)
    } else if let Some(refused) = error.downcast_ref::<NodeError>() {
        (Error::new(refused.code.clone(), format!("{error:#}")), 1)
    } else {
        (Error::new("failed", format!("{error:#}")), 1)
    };
    match json {
        true => eprintln!("{}", serde_json::json!({ "error": shaped })),
        // The message is whole without its code, which is for programs.
        false if matches!(shaped.code.as_str(), "bad_arguments" | "no_node" | "failed") => {
            eprintln!("ahjo: {}", shaped.message)
        }
        false => eprintln!("ahjo: {shaped}"),
    }
    exit
}

async fn run(command: &str, args: &[String]) -> Result<i32> {
    if command == hook::VERB && !asks_help(args) {
        hook::report(args);
        return Ok(0);
    }
    if asks_help(args)
        && let Some(help) = own_help(command)
    {
        println!("{help}");
        return Ok(0);
    }
    match command {
        "" | "-h" | "--help" | "help" => help(args).await,
        "serve" => serve(args).await.map(|()| 0),
        "install" | "upgrade" | "uninstall" => Ok(install::run(command, args).await),
        "--version" | "-V" => {
            println!("ahjo {}", ahjo_node_kit::VERSION);
            Ok(0)
        }
        "version" => version(args).await,
        "kinds" => {
            print!("{}", listing::kinds_table(AREAS));
            Ok(0)
        }
        "mcp" => {
            let (node, areas) = mcp_arguments(args)?;
            let node = node.unwrap_or_else(ahjo_node_kit::address::local);
            mcp::stdio(&node, &areas).await.map(|()| 0)
        }
        flag if flag.starts_with('-') => Err(usage(
            format!("unknown flag {flag}"),
            "ahjo --help lists the commands",
        )),
        noun => cli::run(noun, args, &mut std::io::stdout()).await,
    }
}

/// Every MCP server a node can have: one per area, and the node's own.
fn servers() -> Vec<&'static str> {
    AREAS
        .iter()
        .map(|area| area.name)
        .chain([mcp::NODE])
        .collect()
}

/// The node and the areas of `ahjo mcp`.
fn mcp_arguments(args: &[String]) -> Result<(Option<String>, Vec<String>)> {
    let (mut node, mut areas) = (None, Vec::new());
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = |flag: &str| {
            args.next()
                .filter(|v| !v.is_empty())
                .cloned()
                .ok_or_else(|| usage(format!("{flag} needs a value"), MCP_USAGE))
        };
        match arg.as_str() {
            "--node" => node = Some(value("--node")?),
            "--area" => {
                let area = value("--area")?;
                if !servers().contains(&area.as_str()) {
                    return Err(usage(
                        format!("no area {area}; the areas are {}", servers().join(", ")),
                        MCP_USAGE,
                    ));
                }
                areas.push(area);
            }
            other => return Err(usage(format!("unexpected {other}"), MCP_USAGE)),
        }
    }
    Ok((node, areas))
}

/// `ahjo help`: the usage and the node's commands, or one noun's verbs.
async fn help(args: &[String]) -> Result<i32> {
    let (noun, all, node) = help_arguments(args)?;
    if let Some(noun) = noun {
        if let Some(own) = own_help(noun) {
            println!("{own}");
            return Ok(0);
        }
        let node: Vec<String> = node
            .map(|node| vec!["--node".to_owned(), node.to_owned()])
            .unwrap_or_default();
        return cli::run(noun, &node, &mut std::io::stdout()).await;
    }
    println!("{USAGE}");
    let commands = match all {
        true => cli::overview(node).await.map(|commands| {
            format!("\nEvery command of the running node (ahjo <noun> <verb> --help):\n{commands}")
        }),
        false => cli::index(node).await.map(|nouns| {
            format!(
                "\nThe running node's nouns and their verbs. ahjo <noun> says what each verb \
                 does;\nahjo help --all lists every command.\n{nouns}"
            )
        }),
    };
    match commands {
        Ok(commands) => println!("{commands}"),
        // Help is still help with no node of one's own running.
        Err(error) if node.is_none() && error.is::<Unreachable>() => println!(
            "\nThe node's commands are listed here while it runs: {error:#}.\n\
             Start it with ahjo serve, or put it in place with ahjo install."
        ),
        Err(error) => return Err(error),
    }
    Ok(0)
}

/// The optional noun, `--all` and node of `ahjo help`.
fn help_arguments(args: &[String]) -> Result<(Option<&str>, bool, Option<&str>)> {
    let (mut noun, mut all, mut node) = (None, false, None);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--all" && noun.is_none() {
            all = true;
        } else if arg == "--node" {
            node = Some(
                args.next()
                    .filter(|node| !node.is_empty())
                    .ok_or_else(|| usage("--node needs a node id or URL", HELP_USAGE))?
                    .as_str(),
            );
        } else if let Some(given) = arg.strip_prefix("--node=").filter(|v| !v.is_empty()) {
            node = Some(given);
        } else if arg.starts_with('-') || noun.is_some() || all {
            return Err(usage(format!("unexpected {arg}"), HELP_USAGE));
        } else {
            noun = Some(arg.as_str());
        }
    }
    Ok((noun, all, node))
}

/// This build's version, and the version of the node that answers.
async fn version(args: &[String]) -> Result<i32> {
    let mut node = None;
    let mut json = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--node" => {
                node = Some(
                    args.next()
                        .ok_or_else(|| usage("--node needs addr:port", VERSION_USAGE))?
                        .clone(),
                )
            }
            other => return Err(usage(format!("unexpected {other}"), VERSION_USAGE)),
        }
    }
    let node = node.unwrap_or_else(ahjo_node_kit::address::local);
    let url = match node.contains("://") {
        true => node.clone(),
        false => format!("http://{node}"),
    };
    let client = NodeClient::new(Some(&format!("{}/", url.trim_end_matches('/'))))
        .map_err(|error| usage(format!("--node {node}: {error:#}"), VERSION_USAGE))?;
    let health = client
        .request(axum::http::Method::GET, "/api/health", None, None)
        .await;
    if json {
        let answering = health.as_ref().ok().map(|(health, _)| {
            serde_json::json!({
                "id": health["node"], "address": node, "version": health["version"],
            })
        });
        let answer = serde_json::json!({ "version": ahjo_node_kit::VERSION, "node": answering });
        println!("{}", serde_json::to_string_pretty(&answer)?);
        return Ok(if health.is_ok() { 0 } else { 3 });
    }
    println!("ahjo {}", ahjo_node_kit::VERSION);
    match health {
        Ok((health, _)) => {
            println!(
                "node {} on {node} runs {}",
                health["node"].as_str().unwrap_or("?"),
                health["version"].as_str().unwrap_or("?")
            );
            Ok(0)
        }
        Err(error) if error.is::<Unreachable>() => {
            println!("{error:#}");
            Ok(3)
        }
        Err(error) => {
            println!("the node on {node} did not say: {error:#}");
            Ok(1)
        }
    }
}

/// Serves until Ctrl-C or SIGTERM.
async fn serve(args: &[String]) -> Result<()> {
    let dir = Identity::config_dir()?;
    let mut file = dir.join(config::FILE);
    let mut listen = None;
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| usage(format!("{flag} needs a value"), SERVE_USAGE))
        };
        match flag.as_str() {
            "--config" => file = PathBuf::from(value()?),
            "--listen" => listen = Some(value()?.clone()),
            _ => return Err(usage(format!("unknown flag {flag}"), SERVE_USAGE)),
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
