# ICT (Inner Circle Trader) Strategy

This document provides comprehensive documentation for the ICT (Inner Circle Trader) trading strategy implementation in the Barter-RS ecosystem.

## Overview

The ICT strategy is based on the concepts taught by Michael J. Huddleston (ICT - Inner Circle Trader), focusing on institutional trading patterns and market structure analysis. This implementation includes the core ICT concepts:

- **Fair Value Gaps (FVG)** - Imbalances in price action
- **Order Blocks** - Institutional order zones
- **Market Structure** - Break of Structure (BOS) and Change of Character (CHoCH)
- **Smart Money Concepts** - Following institutional order flow

## Key Features

### Fair Value Gap Detection
Fair Value Gaps are identified when there's a gap between consecutive candles:
- **Bullish FVG**: When candle1.high < candle3.low (gap created by upward movement)
- **Bearish FVG**: When candle1.low > candle3.high (gap created by downward movement)

The strategy tracks these gaps and waits for price to retest them in the direction of the trend.

### Order Block Identification
Order Blocks are identified based on:
- High volume candles
- Significant price range compared to recent averages
- Strong directional movement

These represent areas where institutions placed large orders and can act as support/resistance.

### Market Structure Analysis
The strategy analyzes market structure by:
- Identifying swing highs and lows
- Detecting breaks of structure
- Determining trend direction (Bullish, Bearish, Sideways)

## Configuration

```rust
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
```

### Default Configuration
- `max_candle_history`: 100 candles
- `fvg_min_gap_pips`: 2.0 pips
- `order_block_min_volume`: 1000.0
- `risk_per_trade`: 1%
- `max_position_size`: 1.0

## Usage

### Basic Setup

```rust
use barter::strategy::ict::{IctStrategy, IctConfig, IctInstrumentData};
use barter_execution::order::id::StrategyId;
use rust_decimal_macros::dec;

// Create custom ICT configuration
let ict_config = IctConfig {
    max_candle_history: 200,
    fvg_min_gap_pips: 5.0,
    order_block_min_volume: 2000.0,
    risk_per_trade: dec!(0.02), // 2%
    max_position_size: dec!(0.1),
};

// Create ICT strategy
let ict_strategy = IctStrategy::new(
    StrategyId::new("ict_main_strategy"),
    ict_config,
);
```

### Integration with Barter System

```rust
use barter::system::{
    builder::{SystemArgs, SystemBuilder},
    config::SystemConfig,
};

// Construct System Args with ICT strategy
let args = SystemArgs::new(
    &instruments,
    executions,
    LiveClock,
    ict_strategy,
    DefaultRiskManager::default(),
    market_stream,
    DefaultGlobalData::default(),
    |_| IctInstrumentData::new(Default::default(), Default::default()),
);

// Build system with ICT strategy
let system = SystemBuilder::new(args)
    .engine_feed_mode(EngineFeedMode::Iterator)
    .audit_mode(AuditMode::Enabled)
    .trading_state(TradingState::Disabled)
    .build()?
    .init_with_runtime(tokio::runtime::Handle::current())
    .await?;
```

## Trading Logic

### Entry Signals

#### Fair Value Gap Retest
- **Long Entry**: In a bullish trend, when price retests the bottom of a bullish FVG
- **Short Entry**: In a bearish trend, when price retests the top of a bearish FVG

#### Order Block Retest
- **Long Entry**: In a bullish trend, when price retests a bullish order block (support)
- **Short Entry**: In a bearish trend, when price retests a bearish order block (resistance)

### Risk Management
- Stop loss placed beyond the FVG or Order Block
- Take profit targets based on percentage moves
- Maximum position size enforcement
- Risk per trade percentage limits

## Data Structures

### IctAnalysisData
Maintains the core ICT analysis state:

```rust
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
```

### Fair Value Gap
```rust
pub struct FairValueGap {
    pub top: f64,
    pub bottom: f64,
    pub time: DateTime<Utc>,
    pub direction: FvgDirection, // Bullish or Bearish
    pub filled: bool,
}
```

### Order Block
```rust
pub struct OrderBlock {
    pub high: f64,
    pub low: f64,
    pub time: DateTime<Utc>,
    pub direction: OrderBlockDirection, // Bullish or Bearish
    pub volume: f64,
    pub tested: bool,
}
```

### Market Structure
```rust
pub struct MarketStructure {
    pub trend: TrendDirection, // Bullish, Bearish, Sideways
    pub last_swing_high: Option<SwingPoint>,
    pub last_swing_low: Option<SwingPoint>,
    pub break_of_structure: bool,
}
```

## Example Implementation

See `barter/examples/ict_strategy_example.rs` for a complete working example that demonstrates:

- Setting up the ICT strategy with custom configuration
- Integrating with the Barter trading system
- Processing market data and generating signals
- Monitoring strategy performance

## Testing

The ICT strategy includes comprehensive unit tests:

```bash
cargo test strategy::ict --lib
```

Tests cover:
- Configuration validation
- Fair Value Gap detection
- Strategy instantiation
- Signal generation logic

## Best Practices

### Market Data Requirements
- **Candle Data**: Required for FVG and Order Block detection
- **Timeframes**: Works best with 1m, 5m, or 15m candles
- **History**: Maintain sufficient candle history for pattern recognition

### Configuration Tuning
- **FVG Gap Size**: Adjust based on instrument volatility and timeframe
- **Order Block Volume**: Set threshold based on average volume patterns
- **Risk Parameters**: Align with overall portfolio risk management

### Risk Considerations
- **Trend Following**: Only trades in direction of identified trend
- **Pattern Confirmation**: Waits for retests rather than breakouts
- **Position Sizing**: Respects maximum position limits

## Advanced Features

### Pattern Recognition
The strategy automatically:
- Maintains rolling history of market patterns
- Updates pattern status as new data arrives
- Cleans up old/filled patterns to prevent memory issues

### Signal Quality Filtering
- Only generates signals in trending markets
- Requires pattern confluence for higher probability setups
- Filters based on volume and volatility conditions

## Customization

The ICT strategy can be extended by:

1. **Adding New Patterns**: Implement additional ICT concepts like:
   - Liquidity Sweeps
   - Kill Zones (time-based trading windows)
   - Premium/Discount Arrays

2. **Enhanced Filtering**: Add additional confirmation criteria:
   - Multiple timeframe analysis
   - Volume profile integration
   - Sentiment indicators

3. **Risk Management**: Implement advanced position sizing:
   - Kelly Criterion
   - Volatility-based sizing
   - Correlation-aware limits

## Performance Monitoring

The strategy provides detailed logging and audit trails for:
- Pattern detection events
- Signal generation
- Order execution
- Performance metrics

Use the audit stream to monitor strategy behavior and optimize parameters based on live performance data.

## Disclaimer

This implementation is for educational and research purposes. The ICT strategy involves substantial risk and should only be used with proper risk management and after thorough testing. Always consult with qualified professionals before implementing any trading strategy with real capital.