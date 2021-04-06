use bigdecimal::BigDecimal;
use engine::{Level2View, OrderBook, Side};
use server::{ClientId, OrderId, Price, Quantity, ToClient, ToServer};
use std::{collections::HashMap, io,io::{stdout,Write}};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    sync::mpsc::{self, UnboundedSender},
    task,
    time::Duration,
};

enum ToOrderManager {
    ClientConnected(UnboundedSender<ToClient>),
    ClientDisconnected(ClientId),
    PlaceOrder(ClientId, Side, Price, Quantity),
    GetOrderDepth(ClientId, Side),
    GetTopOfBook(ClientId, Side),
    GetSizeForPriceLevel(ClientId, Side, Price),
}
async fn server_loop(mut events: mpsc::UnboundedReceiver<ToOrderManager>) {
    let mut order_book = OrderBook::default();
    let mut order_counter: OrderId = 0;
    let mut client_counter: ClientId = 0;
    let mut clients: HashMap<ClientId, UnboundedSender<ToClient>> = HashMap::new();
    let mut client_orders: HashMap<ClientId, Vec<OrderId>> = HashMap::new();

    let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            Some(msg) = events.recv() => {
                match msg {
                    ToOrderManager::PlaceOrder(client_id, side, price, quantity) => {
                        order_book.on_new_order(side, price.clone(), quantity, order_counter);
                        let orders = client_orders.entry(client_id).or_insert(vec![]);
                        orders.push(order_counter);

                        order_counter += 1;

                        //ALso send the updated depth to all clients!
                        let quantity = order_book.get_size_for_price_level(side, price.clone());
                        for (_, to_client) in &clients {
                            if let Err(err) =
                                to_client.send(ToClient::LatestDepth(side, quantity, price.as_bigint_and_exponent()))
                            {
                                //Handle error sending to client..
                                println!("Could not send to client {:?}", err);
                            }
                        }
                    }
                    ToOrderManager::ClientConnected(to_client) => {
                        //Cool let assign the new client an id!
                        if let Err(err) = to_client.send(ToClient::Connected(client_counter)) {
                            println!("Could not connect with client.. {:?}", err);
                            continue;
                        }
                        clients.insert(client_counter, to_client);
                        client_counter += 1;
                    }
                    ToOrderManager::ClientDisconnected(client_id) => {
                        //Cleanup all orders
                        if let Some(client_orders) = client_orders.get(&client_id) {
                            for cancel_order in client_orders {
                                order_book.on_cancel_order(*cancel_order);
                            }
                        }
                        clients.remove(&client_id);
                        client_orders.remove(&client_id);
                    }
                    ToOrderManager::GetOrderDepth(client_id,side) => {
                        if let Some(to_client) = clients.get(&client_id) {
                            to_client.send(ToClient::BookDepth(side,order_book.get_book_depth(side)));
                        }
                    }
                    ToOrderManager::GetTopOfBook(client_id,side) => {
                        if let Some(to_client) = clients.get(&client_id) {
                            to_client.send(ToClient::TopOfBook(side,order_book.get_top_of_book(side).as_bigint_and_exponent()));
                        }
                    }
                    ToOrderManager::GetSizeForPriceLevel(client_id,side,price) => {
                        if let Some(to_client) = clients.get(&client_id) {
                            to_client.send(ToClient::SizeForPriceLevel(side,order_book.get_size_for_price_level(side, price)));
                        }
                    }
                }
            }
            _ = heartbeat.tick() => {
                io::stdout().flush().unwrap();
                print!("\rConnected Clients : {:?}",clients.len());
            }
        }
    }
}
async fn client_loop(to_server: UnboundedSender<ToOrderManager>, mut socket: TcpStream) {
    let (client_tx, mut client_rx) = mpsc::unbounded_channel();
    let connect_msg = ToOrderManager::ClientConnected(client_tx);
    if let Err(_) = to_server.send(connect_msg) {
        println!("Could not connect with server");
    }
    let mut client_id: Option<ClientId> = None;
    loop {
        tokio::select! {
            _ = socket.readable()=> {
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
                let to_server_msg: ToServer = bincode::deserialize_from(&buf[0..n]).unwrap();
                match (to_server_msg,client_id) {
                    (ToServer::GetBookDepth(side),Some(client_id)) => {
                        to_server.send(ToOrderManager::GetOrderDepth(client_id,side));
                    },
                    (ToServer::PlaceOrder(side, (digits, scale), quantity),Some(client_id)) => {
                        let price = BigDecimal::new(digits, scale);
                        to_server.send(ToOrderManager::PlaceOrder(client_id, side, price, quantity));

                    },
                    (ToServer::GetTopOfBook(side),Some(client_id)) => {
                        to_server.send(ToOrderManager::GetTopOfBook(client_id,side));
                    },
                    (ToServer::GetSizeForPriceLevel(side,(digits,scale)),Some(client_id)) => {
                        to_server.send(ToOrderManager::GetSizeForPriceLevel(client_id,side,BigDecimal::new(digits,scale)));
                    }
                    _ => ()
                };
            }
            Some(msg) = client_rx.recv() => {
                match msg {
                    ToClient::Connected(our_client_id) => client_id = Some(our_client_id),
                    _ => ()
                }
                socket.write(&bincode::serialize(&msg).unwrap()).await.expect("Could not send to client");
            }
        }
    }
    if let Some(client_id) = client_id {
        to_server.send(ToOrderManager::ClientDisconnected(client_id));
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    let (server_tx, server_rx) = mpsc::unbounded_channel::<ToOrderManager>();
    task::spawn(server_loop(server_rx));
    loop {
        let (socket, _) = listener.accept().await?;
        task::spawn(client_loop(server_tx.clone(), socket));
    }
}
