# hamba timer

![hamba timer](assets/hamba-timer.gif)

a little terminal timer with a pet, work/break tracking, and notes.

## run

```sh
cargo run --release
```

or install it:

```sh
cargo install --path .
hamba_timer
```

## keys

`space` work/break · `s` stop · `n` note · `enter` open/edit
`↑↓` move · `←→` day · `r` resume · `dd` delete · `q` quit

## connect to hermes

```sh
hermes mcp add hamba_timer --command "$(which hamba_timer)" --args serve
hermes mcp test hamba_timer
```

hermes can check your timer, summaries, history, and send messages to the pet.
