use bigdecimal::BigDecimal;
use engine::Side;
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};

/// Protocol for which messages the server can receive
#[derive(Debug, Serialize, Deserialize)]
pub enum ToServer {
    GetBookDepth(engine::Side),
    PlaceOrder(engine::Side, (BigInt, i64), usize),
    GetTopOfBook(engine::Side),
    GetSizeForPriceLevel(engine::Side, (BigInt, i64)),
}

/// Protocol for which messages the server can emit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToClient {
    Connected(ClientId),
    LatestDepth(Side, Quantity, (BigInt, i64)),
    BookDepth(Side, Quantity),
    TopOfBook(Side, (BigInt, i64)),
    SizeForPriceLevel(Side, Quantity),
}

pub type ClientId = usize;
pub type OrderId = usize;
pub type Price = BigDecimal;
pub type Quantity = usize;
