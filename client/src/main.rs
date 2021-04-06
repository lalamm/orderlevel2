use bigdecimal::BigDecimal;
use clap::{App, Arg};
use engine::Side;
use futures::StreamExt;
use rand::prelude::*;
use server::{ToClient, ToServer};
use std::{collections::BTreeMap, error::Error, io, str::FromStr};
use termion::{event::Key, input::MouseTerminal, raw::IntoRawMode, screen::AlternateScreen};
use termion_input_tokio::TermReadAsync;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::{self, Duration};
use tui::{
    backend::TermionBackend,
    layout::{Constraint, Corner, Direction, Layout},
    style::Style,
    text::{Span, Spans},
    widgets::{BarChart, Block, Borders, List, ListItem, Paragraph},
    Terminal,
};
const HELP_TEXT: &str = "Welcome! Here are the commands 
To exit the application : <ESC> 
Place a sell order at asking price 10 and 2 quantities: Ask -p 10 -q 2 
Place a buy order at bidding price 9 and 3 quantities: Bid -p 9.9 -q 3 
Get book depth : Depth -s Ask 
Get Size for price level : Size -s Ask -p 12.2
Get top of book : Top -s Ask
Spam a lot of orders (type loco again to stop) : loco
";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Connect to a peer
    let mut socket = TcpStream::connect("127.0.0.1:8080").await?;
    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut keys_stream = tokio::io::stdin().keys_stream();
    let mut to_client_events = vec![];
    let mut input = String::new();
    let mut is_loco = false;
    let mut loco_timer = time::interval(Duration::from_millis(20));
    let mut bids = BTreeMap::new();
    let mut asks = BTreeMap::new();
    let mut rng = thread_rng();

    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                .split(f.size());

            let left_side = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(80), Constraint::Percentage(20)])
                .split(chunks[0]);
            f.render_widget(Paragraph::new(HELP_TEXT), left_side[0]);

            let paragraph = Paragraph::new(input.as_ref())
                .style(Style::default())
                .block(Block::default().borders(Borders::ALL).title("Input"));
            f.render_widget(paragraph, left_side[1]);

            let right_side = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(chunks[1]);


            let bar_charts_area = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(33),
                    Constraint::Percentage(33),
                    Constraint::Percentage(33),
                ])
                .split(right_side[0]);

            let bids_data = bids
                .iter()
                .map(|(k, v): (&BigDecimal, &usize)| (k.to_string(), *v as u64))
                .collect::<Vec<(String, u64)>>();
            let bids_data_str = &bids_data
                .iter()
                .map(|(k, v)| (k.as_ref(), *v))
                .collect::<Vec<(&str, u64)>>();

            let barchart_bids = BarChart::default()
                .block(Block::default().title("Bids").borders(Borders::ALL))
                .bar_width(7)
                .data(bids_data_str);

            f.render_widget(barchart_bids, bar_charts_area[0]);

            let asks_data = asks
                .iter()
                .map(|(k, v): (&BigDecimal, &usize)| (k.to_string(), *v as u64))
                .collect::<Vec<(String, u64)>>();
            let asks_data_str = &asks_data
                .iter()
                .map(|(k, v)| (k.as_ref(), *v))
                .collect::<Vec<(&str, u64)>>();

            let barchart_asks = BarChart::default()
                .block(Block::default().title("Asks").borders(Borders::ALL))
                .bar_width(7)
                .data(asks_data_str);

            f.render_widget(barchart_asks, bar_charts_area[2]);

            let events: Vec<ListItem> = to_client_events
                .iter()
                .rev()
                .map(|e| {
                    let log = Spans::from(vec![Span::raw(format! {"{:?}",e})]);
                    ListItem::new(vec![Spans::from("-".repeat(chunks[1].width as usize)), log])
                })
                .collect();
            let events_list = List::new(events)
                .block(Block::default().borders(Borders::ALL).title("Events"))
                .start_corner(Corner::BottomLeft);
            f.render_widget(events_list, right_side[1]);
        })?;

        tokio::select! {
            _ = socket.readable() => {
                let mut buf = [0; 1024];
                let n = match socket.try_read(&mut buf){

                    Ok(n) if n == 0 => break,
                    Ok(n) => n,
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        continue;
                    }
                    Err(e) => {
                        println!("failed to read from socket; err = {:?}", e);
                        break;
                    }
                };
                let to_client_msg: ToClient = bincode::deserialize_from(&buf[0..n]).unwrap();
                match to_client_msg.clone(){
                    ToClient::LatestDepth(side,quantity,(digits,exponent)) => {
                        let bhm = match side{
                            Side::Ask => &mut asks,
                            Side::Bid => &mut bids,
                        };
                        let entry = bhm.entry(BigDecimal::new(digits, exponent)).or_insert(0);
                        *entry = quantity;
                    },
                    _ => ()
                }
                to_client_events.push(to_client_msg);
            }
            Some(key) = keys_stream.next() => {
                if let Ok(key) = key{
                    match key {
                        Key::Esc => break,
                        Key::Backspace => {
                            input.pop();
                        },
                        Key::Char('\n') => {
                            if input == "loco"{
                                is_loco = !is_loco;
                            }
                            if let Some(cmd) = try_parse_into_command(&input){
                                socket.write(&bincode::serialize(&cmd).unwrap()).await.expect("Could not send to server");
                            }
                            input.clear();
                        },
                        Key::Char(c) => input.push(c),
                        _ => ()
                    };
                }
            }
            _ = loco_timer.tick() => {
                if is_loco {
                    let side = match rng.gen() {
                        true => Side::Ask,
                        false => Side::Bid
                    };
                    let price = match side {
                        Side::Ask => rng.gen_range(101..103),
                        Side::Bid => rng.gen_range(98..100)
                    };
                    let (digits,exponents) = BigDecimal::from(price).as_bigint_and_exponent();
                    let quantity = rng.gen_range(1..150);
                    socket.write(&bincode::serialize(&ToServer::PlaceOrder(side,(digits,exponents),quantity)).unwrap()).await;
                }
            }

        }
    }
    Ok(())
}
fn try_parse_into_command(input: &str) -> Option<ToServer> {
    let cmd_parser = App::new("client")
        .setting(clap::AppSettings::NoBinaryName)
        .arg(Arg::new("command").requires_ifs(&[("top", "side"), ("depth", "side")]))
        .arg(Arg::new("side").short('s').takes_value(true))
        .arg(Arg::new("price").short('p').takes_value(true))
        .arg(Arg::new("quantity").short('q').takes_value(true));
    if let Ok(parsed) = cmd_parser.try_get_matches_from(input.split(' ')) {
        return match (
            parsed
                .value_of("command")
                .map(|c| c.to_lowercase())
                .as_deref(),
            parsed
                .value_of("price")
                .map(|p| BigDecimal::from_str(p).ok())
                .flatten(),
            parsed
                .value_of("quantity")
                .map(|q| q.parse::<usize>().ok())
                .flatten(),
            parsed
                .value_of("side")
                .map(|s| match s.to_lowercase().as_ref() {
                    "b" | "bid" => Some(engine::Side::Bid),
                    "a" | "ask" => Some(engine::Side::Ask),
                    _ => None,
                })
                .flatten(),
        ) {
            (Some(cmd), Some(price), Some(quantity), _) if cmd == "b" || cmd == "bid" => Some(
                ToServer::PlaceOrder(Side::Bid, price.as_bigint_and_exponent(), quantity),
            ),
            (Some(cmd), Some(price), Some(quantity), _) if cmd == "a" || cmd == "ask" => Some(
                ToServer::PlaceOrder(Side::Ask, price.as_bigint_and_exponent(), quantity),
            ),
            (Some(cmd), _, _, Some(side)) if cmd == "depth" => Some(ToServer::GetBookDepth(side)),
            (Some(cmd), _, _, Some(side)) if cmd == "top" => Some(ToServer::GetTopOfBook(side)),
            (Some(cmd), Some(price), _, Some(side)) if cmd == "size" => Some(
                ToServer::GetSizeForPriceLevel(side, price.as_bigint_and_exponent()),
            ),
            _ => None,
        };
    }
    None
}
