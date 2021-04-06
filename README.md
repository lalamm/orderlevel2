# Level 2 Order Book

**A simple level 2 order book built in Rust.**

---
You can find the order book in the `engine` folder.

## Dependencies
* `Rust v1.51.0` or higher

## Usage 
Run the tests
```
cargo t
```

Look at the documentation
```
cargo doc --open
```

## Extra: Cli
Since it felt boring with an empty order book I've also created a simple cli. To use it, start a server in one terminal window and at least one client in another window.

Start a server
```
cargo r --bin server --release
```

Start a client
```
cargo r --bin client --release
```
The cli can be used to place bids and orders but not for traiding.

Here's a gif showing the cli with one server and three clients
![](trading_cli.gif)

cheerio