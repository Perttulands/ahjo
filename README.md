# Ahjo

Ahjo is a family of three apps on one app kit: Ahjo Swarm, where you run your
agents; Ahjo Brain, for exploring what we know; and Ahjo Desk, where agents
publish and you answer.

Ahjo Swarm holds sessions, files, work, brain, host and missions in one Rust
binary, `ahjo`, with a native app for you and a CLI for your agent.

`VISION.md` says what Swarm is for. `ARCHITECTURE.md` says how it is built.

To run it on Linux or WSL, build `cargo build --release -p ahjo -p ahjo-app-swarm`,
then `target/release/ahjo install --app target/release/ahjo-swarm` makes the
node the systemd user unit `ahjo`, and a later build goes in with
`target/release/ahjo upgrade --app target/release/ahjo-swarm`; terminal
sessions live on through both.
