//! An implementation of a level 2 order view

use bigdecimal::BigDecimal;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Side of the trade
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum Side {
    Bid,
    Ask,
}
type OrderId = usize;
type Quantity = usize;

pub trait Level2View {
    fn on_new_order(&mut self, side: Side, price: BigDecimal, quantity: usize, order_id: usize);
    fn on_cancel_order(&mut self, order_id: usize);
    fn on_replace_order(&mut self, price: BigDecimal, quantity: Quantity, order_id: usize);
    // When an aggressor order crosses the spread, it will be matched with an existing resting order, causing a trade.
    // The aggressor order will NOT cause an invocation of onNewOrder.
    fn on_trade(&mut self, quantity: usize, resting_order_id: usize);
    fn get_size_for_price_level(&mut self, side: Side, price: BigDecimal) -> usize;
    fn get_book_depth(&self, side: Side) -> usize;
    fn get_top_of_book(&self, side: Side) -> BigDecimal;
}

/// BTreeMap looks like a good fit when reading [here](https://doc.rust-lang.org/std/collections/index.html)
#[derive(Default)]
pub struct OrderBook {
    bids: BTreeMap<BigDecimal, Quantity>,
    asks: BTreeMap<BigDecimal, Quantity>,
    orders: HashMap<OrderId, (Side, BigDecimal, Quantity)>,
}

impl Level2View for OrderBook {
    fn on_new_order(
        &mut self,
        side: Side,
        price: BigDecimal,
        quantity: Quantity,
        order_id: OrderId,
    ) {
        let book = match side {
            Side::Ask => &mut self.asks,
            Side::Bid => &mut self.bids,
        };
        let order_depth = book.entry(price.clone()).or_insert(0);
        *order_depth += quantity;
        //Would like to use unstable here.. https://github.com/rust-lang/rust/issues/62633
        if self.orders.insert(order_id, (side, price, quantity)).is_some() {
            panic!("Order id is {} already present", order_id);
        }
    }

    fn on_cancel_order(&mut self, order_id: usize) {
        let (side, price, quantity) = self
            .orders
            .remove(&order_id)
            .unwrap_or_else(|| panic!("Missing order_id {}", order_id));

        let order_depth = match side {
            Side::Ask => &mut self.asks,
            Side::Bid => &mut self.bids,
        }
        .get_mut(&price)
        .expect("Order was not in the order book");
        *order_depth -= quantity;

        if *order_depth == 0 {
            match side {
                Side::Ask => &mut self.asks,
                Side::Bid => &mut self.bids,
            }
            .remove(&price);
        }
    }

    fn on_replace_order(&mut self, price: BigDecimal, quantity: Quantity, order_id: usize) {
        let current_order_side = self
            .orders
            .get(&order_id)
            .unwrap_or_else( || panic!("Can't replace non existing order {}", order_id))
            .0;
        self.on_cancel_order(order_id);
        self.on_new_order(current_order_side, price, quantity, order_id);
    }
    fn on_trade(&mut self, quantity: usize, resting_order_id: usize) {
        let (side, price, resting_quantity) = self.orders.get_mut(&resting_order_id).unwrap_or_else(
            || panic!("Resting order id did not exist {}", resting_order_id),
        );

        *resting_quantity = resting_quantity.checked_sub(quantity).expect("Can't trade more than available quantity");

        //Also subtract from book
        let book = match side {
            Side::Ask => &mut self.asks,
            Side::Bid => &mut self.bids,
        };
        let order_depth = book.get_mut(price).expect("Price depth did not exist");
        *order_depth -= quantity;
    }

    fn get_size_for_price_level(&mut self, side: Side, price: BigDecimal) -> Quantity {
        *match side {
            Side::Ask => &self.asks,
            Side::Bid => &self.bids,
        }
        .get(&price)
        .unwrap_or_else(|| panic!("Price level did not exist {}", price))
    }

    fn get_book_depth(&self, side: Side) -> usize {
        match side {
            Side::Ask => self.asks.len(),
            Side::Bid => self.bids.len(),
        }
    }

    fn get_top_of_book(&self, side: Side) -> BigDecimal {
        // TODO:implement When merged into stable rust  https://github.com/rust-lang/rust/issues/62924
        match side {
            Side::Bid => self.bids.iter().rev().next(),
            Side::Ask => self.asks.iter().next(),
        }
        .expect("Order book is empty")
        .0
        .clone() //Does not impl copy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn add_new_order() {
        let mut order_book = OrderBook::default();
        order_book.on_new_order(Side::Ask, 12.into(), 5, 1);
        assert_eq!(order_book.get_size_for_price_level(Side::Ask, 12.into()), 5);
        assert_eq!(order_book.get_book_depth(Side::Ask), 1);
        assert_eq!(order_book.get_top_of_book(Side::Ask), 12.into());

        order_book.on_new_order(Side::Bid, 11.into(), 3, 2);
        assert_eq!(order_book.get_size_for_price_level(Side::Bid, 11.into()), 3);
        assert_eq!(order_book.get_book_depth(Side::Bid), 1);
        assert_eq!(order_book.get_top_of_book(Side::Bid), 11.into());
    }

    #[test]
    fn trade() {
        let mut order_book = OrderBook::default();
        order_book.on_new_order(Side::Ask, 12.into(), 5, 1);
        order_book.on_trade(4, 1);
        assert_eq!(order_book.get_size_for_price_level(Side::Ask, 12.into()), 1);
    }

    #[test]
    #[should_panic]
    fn trade_more_than_available() {
        let mut order_book = OrderBook::default();
        order_book.on_new_order(Side::Ask, 12.into(), 5, 1);
        order_book.on_trade(6, 1);
    }

    #[test]
    fn replace_order() {
        let mut order_book = OrderBook::default();
        order_book.on_new_order(Side::Ask, 12.into(), 5, 1);
        assert_eq!(order_book.get_size_for_price_level(Side::Ask, 12.into()), 5);
        order_book.on_replace_order(12.into(), 1, 1);
        assert_eq!(order_book.get_size_for_price_level(Side::Ask, 12.into()), 1);
    }

    #[test]
    fn cancel_order() {
        let mut order_book = OrderBook::default();
        order_book.on_new_order(Side::Ask, 12.into(), 1, 1);
        order_book.on_new_order(Side::Ask, 12.into(), 2, 2);
        assert_eq!(order_book.get_size_for_price_level(Side::Ask, 12.into()), 3);
        order_book.on_cancel_order(1);
        assert_eq!(order_book.get_size_for_price_level(Side::Ask, 12.into()), 2);
    }

    #[test]
    #[should_panic]
    fn test_invalid_cancel_twice() {
        let mut order_book = OrderBook::default();
        order_book.on_new_order(Side::Ask, 12.into(), 5, 1);
        order_book.on_cancel_order(1);
        order_book.on_cancel_order(1);
    }
}
