# ICT Strategy for Barter-RS

This repository now includes a comprehensive **ICT (Inner Circle Trader) strategy** implementation based on the concepts taught by Michael J. Huddleston.

## New ICT Strategy Features

- **Fair Value Gap (FVG) Detection** - Automatically identifies price imbalances
- **Order Block Identification** - Detects institutional order zones
- **Market Structure Analysis** - Tracks Break of Structure (BOS) and trend direction
- **Smart Entry Logic** - Enters trades on retests of key levels in trend direction

## Quick Start with ICT Strategy

```rust
use barter::strategy::ict::{IctStrategy, IctConfig};
use barter_execution::order::id::StrategyId;
use rust_decimal_macros::dec;

// Create ICT strategy with custom settings
let ict_config = IctConfig {
    max_candle_history: 200,
    fvg_min_gap_pips: 5.0,
    order_block_min_volume: 2000.0,
    risk_per_trade: dec!(0.02), // 2% risk
    max_position_size: dec!(0.1),
};

let ict_strategy = IctStrategy::new(
    StrategyId::new("ict_main_strategy"),
    ict_config,
);

// Use with your trading system
let args = SystemArgs::new(
    &instruments,
    executions,
    LiveClock,
    ict_strategy, // <- Use ICT strategy here
    DefaultRiskManager::default(),
    market_stream,
    // ... other args
);
```

## Examples and Documentation

- **Example**: `barter/examples/ict_strategy_example.rs` - Complete working example
- **Documentation**: `ICT_STRATEGY.md` - Comprehensive strategy documentation
- **Tests**: Run `cargo test strategy::ict` for unit tests

## ICT Strategy Concepts Implemented

| Concept | Description | Implementation |
|---------|-------------|----------------|
| Fair Value Gaps | Price imbalances between candles | Automatic detection and tracking |
| Order Blocks | High-volume institutional zones | Volume and range-based identification |
| Market Structure | Trend analysis via swing points | Higher highs/lows pattern recognition |
| Smart Entries | Retest-based entry logic | FVG and Order Block retest signals |

The ICT strategy integrates seamlessly with the existing Barter framework and supports all standard strategy interfaces including position management, risk controls, and audit logging.

For detailed usage instructions, see the comprehensive documentation in `ICT_STRATEGY.md`.