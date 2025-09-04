use crate::{
    engine::{
        Processor,
        state::{
            EngineState,
            instrument::{data::{DefaultInstrumentMarketData, InstrumentDataState}, filter::InstrumentFilter},
            order::in_flight_recorder::InFlightRequestRecorder,
        },
    },
    strategy::{
        algo::AlgoStrategy,
        close_positions::ClosePositionsStrategy,
        on_disconnect::OnDisconnectStrategy,
        on_trading_disabled::OnTradingDisabled,
    },
};
use barter_data::{
    event::{DataKind, MarketEvent},
    subscription::candle::Candle,
};
use barter_execution::{
    AccountEvent,
    order::{
        id::{ClientOrderId, StrategyId},
        request::{OrderRequestCancel, OrderRequestOpen, RequestOpen},
        OrderKey, OrderKind, TimeInForce,
    },
};
use barter_instrument::{
    Side,
    asset::AssetIndex,
    exchange::{ExchangeId, ExchangeIndex},
    instrument::InstrumentIndex,
};
use chrono::{DateTime, Utc};
use derive_more::Constructor;
use rust_decimal::{Decimal, prelude::{FromPrimitive, ToPrimitive}};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, fmt::Debug};

/// Inner Circle Trader (ICT) strategy implementation.
///
/// This strategy implements core ICT concepts including:
/// - Fair Value Gap (FVG) detection
/// - Order Block identification  
/// - Basic market structure analysis
/// - Entry logic based on ICT principles
///
/// The strategy maintains historical candle data to identify patterns and make trading decisions.
#[derive(Debug, Clone)]
pub struct IctStrategy {
    pub id: StrategyId,
    pub config: IctConfig,
}

impl Default for IctStrategy {
    fn default() -> Self {
        Self {
            id: StrategyId::new("ict_strategy"),
            config: IctConfig::default(),
        }
    }
}

impl IctStrategy {
    pub fn new(id: StrategyId, config: IctConfig) -> Self {
        Self { id, config }
    }
}

/// Configuration for the ICT strategy
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IctConfig {
    /// Maximum number of candles to maintain in history
    pub max_candle_history: usize,
    /// Minimum gap size in pips to consider a Fair Value Gap
    pub fvg_min_gap_pips: f64,
    /// Minimum volume for order block identification
    pub order_block_min_volume: f64,
    /// Risk percentage per trade
    pub risk_per_trade: Decimal,
    /// Maximum position size
    pub max_position_size: Decimal,
}

impl Default for IctConfig {
    fn default() -> Self {
        Self {
            max_candle_history: 100,
            fvg_min_gap_pips: 2.0,
            order_block_min_volume: 1000.0,
            risk_per_trade: Decimal::from_str_exact("0.01").unwrap(), // 1%
            max_position_size: Decimal::from_str_exact("1.0").unwrap(),
        }
    }
}

/// Custom instrument data for the ICT strategy
#[derive(Debug, Clone, Constructor)]
pub struct IctInstrumentData {
    /// Standard market data
    pub market_data: DefaultInstrumentMarketData,
    /// ICT-specific data
    pub ict_data: IctAnalysisData,
}

impl Default for IctInstrumentData {
    fn default() -> Self {
        Self {
            market_data: Default::default(),
            ict_data: IctAnalysisData::default(),
        }
    }
}

/// ICT analysis data structure
#[derive(Debug, Clone, Default)]
pub struct IctAnalysisData {
    /// Historical candles for pattern analysis
    pub candle_history: VecDeque<Candle>,
    /// Identified Fair Value Gaps
    pub fair_value_gaps: Vec<FairValueGap>,
    /// Identified Order Blocks
    pub order_blocks: Vec<OrderBlock>,
    /// Current market structure state
    pub market_structure: MarketStructure,
    /// Last processed candle time to avoid duplicates
    pub last_candle_time: Option<DateTime<Utc>>,
}

/// Fair Value Gap structure
#[derive(Debug, Clone, PartialEq)]
pub struct FairValueGap {
    pub top: f64,
    pub bottom: f64,
    pub time: DateTime<Utc>,
    pub direction: FvgDirection,
    pub filled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FvgDirection {
    Bullish,  // Gap created by upward movement
    Bearish,  // Gap created by downward movement
}

/// Order Block structure
#[derive(Debug, Clone, PartialEq)]
pub struct OrderBlock {
    pub high: f64,
    pub low: f64,
    pub time: DateTime<Utc>,
    pub direction: OrderBlockDirection,
    pub volume: f64,
    pub tested: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OrderBlockDirection {
    Bullish,  // Support level
    Bearish,  // Resistance level
}

/// Market structure analysis
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MarketStructure {
    pub trend: TrendDirection,
    pub last_swing_high: Option<SwingPoint>,
    pub last_swing_low: Option<SwingPoint>,
    pub break_of_structure: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrendDirection {
    Bullish,
    Bearish,
    Sideways,
}

impl Default for TrendDirection {
    fn default() -> Self {
        TrendDirection::Sideways
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwingPoint {
    pub price: f64,
    pub time: DateTime<Utc>,
}

impl IctAnalysisData {
    /// Add a new candle and perform ICT analysis
    pub fn process_candle(&mut self, candle: &Candle, config: &IctConfig) {
        // Avoid processing the same candle multiple times
        if self.last_candle_time.map_or(false, |last| last >= candle.close_time) {
            return;
        }

        // Add candle to history
        self.candle_history.push_back(*candle);
        
        // Maintain maximum history size
        while self.candle_history.len() > config.max_candle_history {
            self.candle_history.pop_front();
        }

        self.last_candle_time = Some(candle.close_time);

        // Perform ICT analysis if we have enough data
        if self.candle_history.len() >= 3 {
            self.detect_fair_value_gaps(config);
            self.identify_order_blocks(config);
            self.update_market_structure();
        }
    }

    /// Detect Fair Value Gaps in the candle history
    fn detect_fair_value_gaps(&mut self, config: &IctConfig) {
        let candles: Vec<_> = self.candle_history.iter().collect();
        
        // Need at least 3 candles to detect FVG
        if candles.len() < 3 {
            return;
        }

        // Check the last 3 candles for FVG pattern
        let len = candles.len();
        if len >= 3 {
            let candle1 = candles[len - 3];
            let candle2 = candles[len - 2]; // Gap candle
            let candle3 = candles[len - 1];

            // Bullish FVG: candle1.high < candle3.low (gap between them)
            if candle1.high < candle3.low {
                let gap_size = candle3.low - candle1.high;
                if gap_size >= config.fvg_min_gap_pips / 10000.0 { // Convert pips to price
                    let fvg = FairValueGap {
                        top: candle3.low,
                        bottom: candle1.high,
                        time: candle2.close_time,
                        direction: FvgDirection::Bullish,
                        filled: false,
                    };
                    
                    // Only add if not already exists
                    if !self.fair_value_gaps.iter().any(|existing| 
                        existing.time == fvg.time && existing.direction == fvg.direction
                    ) {
                        self.fair_value_gaps.push(fvg);
                    }
                }
            }

            // Bearish FVG: candle1.low > candle3.high (gap between them)
            if candle1.low > candle3.high {
                let gap_size = candle1.low - candle3.high;
                if gap_size >= config.fvg_min_gap_pips / 10000.0 { // Convert pips to price
                    let fvg = FairValueGap {
                        top: candle1.low,
                        bottom: candle3.high,
                        time: candle2.close_time,
                        direction: FvgDirection::Bearish,
                        filled: false,
                    };
                    
                    // Only add if not already exists
                    if !self.fair_value_gaps.iter().any(|existing| 
                        existing.time == fvg.time && existing.direction == fvg.direction
                    ) {
                        self.fair_value_gaps.push(fvg);
                    }
                }
            }
        }

        // Update FVG filled status based on current price
        if let Some(current_candle) = candles.last() {
            for fvg in &mut self.fair_value_gaps {
                if !fvg.filled {
                    match fvg.direction {
                        FvgDirection::Bullish => {
                            // Bullish FVG is filled when price goes back down into the gap
                            if current_candle.low <= fvg.bottom {
                                fvg.filled = true;
                            }
                        }
                        FvgDirection::Bearish => {
                            // Bearish FVG is filled when price goes back up into the gap
                            if current_candle.high >= fvg.top {
                                fvg.filled = true;
                            }
                        }
                    }
                }
            }
        }

        // Clean up old FVGs (keep only last 20)
        if self.fair_value_gaps.len() > 20 {
            self.fair_value_gaps.drain(0..self.fair_value_gaps.len() - 20);
        }
    }

    /// Identify Order Blocks based on volume and price action
    fn identify_order_blocks(&mut self, config: &IctConfig) {
        let candles: Vec<_> = self.candle_history.iter().collect();
        
        // Need at least 5 candles for order block identification
        if candles.len() < 5 {
            return;
        }

        // Look for order blocks in recent candles
        let len = candles.len();
        for i in 2..len.saturating_sub(2) {
            let candle = candles[i];
            
            // Order block criteria: High volume candle with significant price move
            if candle.volume >= config.order_block_min_volume {
                let price_range = candle.high - candle.low;
                let avg_range = self.calculate_average_range(&candles, i, 10);
                
                // Order block if price range is significantly larger than average
                if price_range > avg_range * 1.5 {
                    // Determine direction based on close relative to open
                    let direction = if candle.close > candle.open {
                        OrderBlockDirection::Bullish
                    } else {
                        OrderBlockDirection::Bearish
                    };

                    let order_block = OrderBlock {
                        high: candle.high,
                        low: candle.low,
                        time: candle.close_time,
                        direction,
                        volume: candle.volume,
                        tested: false,
                    };

                    // Only add if not already exists
                    if !self.order_blocks.iter().any(|existing| 
                        existing.time == order_block.time
                    ) {
                        self.order_blocks.push(order_block);
                    }
                }
            }
        }

        // Update order block tested status
        if let Some(current_candle) = candles.last() {
            for order_block in &mut self.order_blocks {
                if !order_block.tested {
                    match order_block.direction {
                        OrderBlockDirection::Bullish => {
                            // Bullish order block is tested when price comes back to the low
                            if current_candle.low <= order_block.low * 1.001 { // Small tolerance
                                order_block.tested = true;
                            }
                        }
                        OrderBlockDirection::Bearish => {
                            // Bearish order block is tested when price comes back to the high
                            if current_candle.high >= order_block.high * 0.999 { // Small tolerance
                                order_block.tested = true;
                            }
                        }
                    }
                }
            }
        }

        // Clean up old order blocks (keep only last 15)
        if self.order_blocks.len() > 15 {
            self.order_blocks.drain(0..self.order_blocks.len() - 15);
        }
    }

    /// Calculate average range for order block detection
    fn calculate_average_range(&self, candles: &[&Candle], center: usize, lookback: usize) -> f64 {
        let start = center.saturating_sub(lookback / 2);
        let end = (center + lookback / 2).min(candles.len());
        
        let total_range: f64 = candles[start..end]
            .iter()
            .map(|candle| candle.high - candle.low)
            .sum();
            
        total_range / (end - start) as f64
    }

    /// Update market structure based on swing points
    fn update_market_structure(&mut self) {
        let candles: Vec<_> = self.candle_history.iter().collect();
        
        if candles.len() < 5 {
            return;
        }

        // Identify swing highs and lows
        let len = candles.len();
        for i in 2..len.saturating_sub(2) {
            let candle = candles[i];
            let prev2 = candles[i - 2];
            let prev1 = candles[i - 1];
            let next1 = candles[i + 1];
            let next2 = candles[i + 2];

            // Swing high: higher than surrounding candles
            if candle.high > prev2.high && candle.high > prev1.high && 
               candle.high > next1.high && candle.high > next2.high {
                let swing_high = SwingPoint {
                    price: candle.high,
                    time: candle.close_time,
                };

                // Check for break of structure
                if let Some(ref last_high) = self.market_structure.last_swing_high {
                    if swing_high.price > last_high.price {
                        self.market_structure.break_of_structure = true;
                        self.market_structure.trend = TrendDirection::Bullish;
                    }
                }
                
                self.market_structure.last_swing_high = Some(swing_high);
            }

            // Swing low: lower than surrounding candles
            if candle.low < prev2.low && candle.low < prev1.low && 
               candle.low < next1.low && candle.low < next2.low {
                let swing_low = SwingPoint {
                    price: candle.low,
                    time: candle.close_time,
                };

                // Check for break of structure
                if let Some(ref last_low) = self.market_structure.last_swing_low {
                    if swing_low.price < last_low.price {
                        self.market_structure.break_of_structure = true;
                        self.market_structure.trend = TrendDirection::Bearish;
                    }
                }
                
                self.market_structure.last_swing_low = Some(swing_low);
            }
        }
    }

    /// Generate trading signal based on ICT analysis
    pub fn generate_signal(&self, current_price: f64, _config: &IctConfig) -> Option<IctSignal> {
        // Look for entry opportunities based on FVG retest in trend direction
        for fvg in &self.fair_value_gaps {
            if !fvg.filled {
                match (&self.market_structure.trend, &fvg.direction) {
                    (TrendDirection::Bullish, FvgDirection::Bullish) => {
                        // In bullish trend, look for price to retest bullish FVG bottom
                        if current_price <= fvg.bottom && current_price >= fvg.bottom * 0.999 {
                            return Some(IctSignal::Long {
                                entry_price: current_price,
                                stop_loss: fvg.bottom * 0.995, // 0.5% below FVG
                                take_profit: current_price * 1.02, // 2% target
                                reason: "Bullish FVG retest in uptrend".to_string(),
                            });
                        }
                    }
                    (TrendDirection::Bearish, FvgDirection::Bearish) => {
                        // In bearish trend, look for price to retest bearish FVG top
                        if current_price >= fvg.top && current_price <= fvg.top * 1.001 {
                            return Some(IctSignal::Short {
                                entry_price: current_price,
                                stop_loss: fvg.top * 1.005, // 0.5% above FVG
                                take_profit: current_price * 0.98, // 2% target
                                reason: "Bearish FVG retest in downtrend".to_string(),
                            });
                        }
                    }
                    _ => {} // Don't trade against trend
                }
            }
        }

        // Look for order block retest opportunities
        for order_block in &self.order_blocks {
            if !order_block.tested {
                match (&self.market_structure.trend, &order_block.direction) {
                    (TrendDirection::Bullish, OrderBlockDirection::Bullish) => {
                        // In bullish trend, look for price to retest bullish order block
                        if current_price <= order_block.low * 1.002 && current_price >= order_block.low * 0.998 {
                            return Some(IctSignal::Long {
                                entry_price: current_price,
                                stop_loss: order_block.low * 0.99, // 1% below order block
                                take_profit: current_price * 1.03, // 3% target
                                reason: "Bullish order block retest in uptrend".to_string(),
                            });
                        }
                    }
                    (TrendDirection::Bearish, OrderBlockDirection::Bearish) => {
                        // In bearish trend, look for price to retest bearish order block
                        if current_price >= order_block.high * 0.998 && current_price <= order_block.high * 1.002 {
                            return Some(IctSignal::Short {
                                entry_price: current_price,
                                stop_loss: order_block.high * 1.01, // 1% above order block
                                take_profit: current_price * 0.97, // 3% target
                                reason: "Bearish order block retest in downtrend".to_string(),
                            });
                        }
                    }
                    _ => {} // Don't trade against trend
                }
            }
        }

        None
    }
}

/// ICT trading signal
#[derive(Debug, Clone)]
pub enum IctSignal {
    Long {
        entry_price: f64,
        stop_loss: f64,
        take_profit: f64,
        reason: String,
    },
    Short {
        entry_price: f64,
        stop_loss: f64,
        take_profit: f64,
        reason: String,
    },
}

// Implement required traits for IctInstrumentData

impl InstrumentDataState for IctInstrumentData {
    type MarketEventKind = DataKind;

    fn price(&self) -> Option<Decimal> {
        self.market_data.price()
    }
}

impl<InstrumentKey> Processor<&MarketEvent<InstrumentKey, DataKind>> for IctInstrumentData {
    type Audit = ();

    fn process(&mut self, event: &MarketEvent<InstrumentKey, DataKind>) -> Self::Audit {
        // Process standard market data
        self.market_data.process(event);

        // Process ICT-specific data
        match &event.kind {
            DataKind::Candle(candle) => {
                // Process candle data for ICT analysis with default config
                let config = IctConfig::default();
                self.ict_data.process_candle(candle, &config);
            }
            DataKind::Trade(_trade) => {
                // We can construct candles from trades if needed
                // For this demo, we'll skip this complex logic
            }
            _ => {}
        }
    }
}

impl<ExchangeKey, AssetKey, InstrumentKey> Processor<&AccountEvent<ExchangeKey, AssetKey, InstrumentKey>> for IctInstrumentData {
    type Audit = ();

    fn process(&mut self, event: &AccountEvent<ExchangeKey, AssetKey, InstrumentKey>) -> Self::Audit {
        // Delegate to standard market data processor
        self.market_data.process(event)
    }
}

impl<ExchangeKey, InstrumentKey> InFlightRequestRecorder<ExchangeKey, InstrumentKey> for IctInstrumentData {
    fn record_in_flight_cancel(&mut self, request: &OrderRequestCancel<ExchangeKey, InstrumentKey>) {
        self.market_data.record_in_flight_cancel(request)
    }

    fn record_in_flight_open(&mut self, request: &OrderRequestOpen<ExchangeKey, InstrumentKey>) {
        self.market_data.record_in_flight_open(request)
    }
}

// Implement strategy traits

impl AlgoStrategy<ExchangeIndex, InstrumentIndex> for IctStrategy {
    type State = EngineState<(), IctInstrumentData>;

    fn generate_algo_orders(
        &self,
        state: &Self::State,
    ) -> (
        impl IntoIterator<Item = OrderRequestCancel<ExchangeIndex, InstrumentIndex>>,
        impl IntoIterator<Item = OrderRequestOpen<ExchangeIndex, InstrumentIndex>>,
    ) {
        let mut open_orders = Vec::new();
        
        // Iterate through all instruments and generate orders based on ICT signals
        for instrument_state in state.instruments.instruments(&InstrumentFilter::None) {
            if let Some(current_price) = instrument_state.data.price() {
                let current_price_f64 = current_price.to_f64().unwrap_or(0.0);
                
                // Generate signal based on ICT analysis
                if let Some(signal) = instrument_state.data.ict_data.generate_signal(current_price_f64, &self.config) {
                    match signal {
                        IctSignal::Long { entry_price, stop_loss: _, take_profit: _, reason: _ } => {
                            if let Some(entry_decimal) = Decimal::from_f64(entry_price) {
                                let order_key = OrderKey::new(
                                    instrument_state.instrument.exchange,
                                    instrument_state.key,
                                    self.id.clone(),
                                    ClientOrderId::random(),
                                );
                                
                                let request_open = RequestOpen::new(
                                    Side::Buy,
                                    entry_decimal,
                                    self.config.max_position_size,
                                    OrderKind::Market,
                                    TimeInForce::ImmediateOrCancel,
                                );

                                let order = OrderRequestOpen::new(order_key, request_open);
                                open_orders.push(order);
                            }
                        }
                        IctSignal::Short { entry_price, stop_loss: _, take_profit: _, reason: _ } => {
                            if let Some(entry_decimal) = Decimal::from_f64(entry_price) {
                                let order_key = OrderKey::new(
                                    instrument_state.instrument.exchange,
                                    instrument_state.key,
                                    self.id.clone(),
                                    ClientOrderId::random(),
                                );
                                
                                let request_open = RequestOpen::new(
                                    Side::Sell,
                                    entry_decimal,
                                    self.config.max_position_size,
                                    OrderKind::Market,
                                    TimeInForce::ImmediateOrCancel,
                                );

                                let order = OrderRequestOpen::new(order_key, request_open);
                                open_orders.push(order);
                            }
                        }
                    }
                }
            }
        }

        (std::iter::empty(), open_orders)
    }
}

impl ClosePositionsStrategy for IctStrategy {
    type State = EngineState<(), IctInstrumentData>;

    fn close_positions_requests<'a>(
        &'a self,
        state: &'a Self::State,
        filter: &'a InstrumentFilter,
    ) -> (
        impl IntoIterator<Item = OrderRequestCancel<ExchangeIndex, InstrumentIndex>> + 'a,
        impl IntoIterator<Item = OrderRequestOpen<ExchangeIndex, InstrumentIndex>> + 'a,
    )
    where
        ExchangeIndex: 'a,
        AssetIndex: 'a,
        InstrumentIndex: 'a,
    {
        // Generate MARKET orders to close all open positions
        let open_requests = state
            .instruments
            .instruments(filter)
            .filter_map(move |instrument_state| {
                // Get current market price
                let _price = instrument_state.data.price()?;
                
                // For this simplified implementation, we'll just use a placeholder
                // In a real implementation, you'd track positions and generate appropriate close orders
                None
            });

        (std::iter::empty(), open_requests)
    }
}

impl<Clock, State, ExecutionTxs, Risk> OnDisconnectStrategy<Clock, State, ExecutionTxs, Risk> for IctStrategy {
    type OnDisconnect = ();

    fn on_disconnect(
        _: &mut crate::engine::Engine<Clock, State, ExecutionTxs, Self, Risk>,
        _: ExchangeId,
    ) -> Self::OnDisconnect {
        // Log disconnection or implement custom logic
    }
}

impl<Clock, State, ExecutionTxs, Risk> OnTradingDisabled<Clock, State, ExecutionTxs, Risk> for IctStrategy {
    type OnTradingDisabled = ();

    fn on_trading_disabled(
        _: &mut crate::engine::Engine<Clock, State, ExecutionTxs, Self, Risk>,
    ) -> Self::OnTradingDisabled {
        // Log trading disabled or implement custom logic
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_ict_config_default() {
        let config = IctConfig::default();
        assert_eq!(config.max_candle_history, 100);
        assert_eq!(config.fvg_min_gap_pips, 2.0);
        assert_eq!(config.order_block_min_volume, 1000.0);
    }

    #[test]
    fn test_fair_value_gap_detection() {
        let mut ict_data = IctAnalysisData::default();
        let config = IctConfig::default();

        // Create test candles that form a bullish FVG
        let candle1 = Candle {
            close_time: Utc::now(),
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.5,
            volume: 1000.0,
            trade_count: 10,
        };

        let candle2 = Candle {
            close_time: Utc::now(),
            open: 100.5,
            high: 106.0,
            low: 100.0,
            close: 105.0,
            volume: 2000.0,
            trade_count: 20,
        };

        let candle3 = Candle {
            close_time: Utc::now(),
            open: 105.0,
            high: 107.0,
            low: 104.0, // This creates a gap with candle1.high (101.0)
            close: 106.0,
            volume: 1500.0,
            trade_count: 15,
        };

        ict_data.process_candle(&candle1, &config);
        ict_data.process_candle(&candle2, &config);
        ict_data.process_candle(&candle3, &config);

        // Should detect a bullish FVG
        assert_eq!(ict_data.fair_value_gaps.len(), 1);
        assert_eq!(ict_data.fair_value_gaps[0].direction, FvgDirection::Bullish);
        assert_eq!(ict_data.fair_value_gaps[0].top, 104.0);
        assert_eq!(ict_data.fair_value_gaps[0].bottom, 101.0);
    }

    #[test]
    fn test_ict_strategy_creation() {
        let strategy = IctStrategy::default();
        assert_eq!(strategy.id.0.as_str(), "ict_strategy");
        
        let custom_config = IctConfig {
            max_candle_history: 50,
            fvg_min_gap_pips: 1.0,
            order_block_min_volume: 500.0,
            risk_per_trade: Decimal::from_str_exact("0.02").unwrap(),
            max_position_size: Decimal::from_str_exact("0.5").unwrap(),
        };
        
        let custom_strategy = IctStrategy::new(
            StrategyId::new("custom_ict"),
            custom_config.clone()
        );
        
        assert_eq!(custom_strategy.id.0.as_str(), "custom_ict");
        assert_eq!(custom_strategy.config.max_candle_history, 50);
    }
}