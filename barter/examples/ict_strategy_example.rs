use barter::{
    EngineEvent,
    engine::{
        Engine, Processor,
        clock::LiveClock,
        state::{
            EngineState,
            global::DefaultGlobalData,
            instrument::{
                data::InstrumentDataState,
                filter::InstrumentFilter,
            },
            trading::TradingState,
        },
    },
    logging::init_logging,
    risk::DefaultRiskManager,
    statistic::{summary::instrument::TearSheetGenerator, time::Daily},
    strategy::{
        ict::{IctStrategy, IctConfig, IctInstrumentData},
        close_positions::ClosePositionsStrategy,
        on_disconnect::OnDisconnectStrategy,
        on_trading_disabled::OnTradingDisabled,
    },
    system::{
        builder::{AuditMode, EngineFeedMode, SystemArgs, SystemBuilder},
        config::SystemConfig,
    },
};
use barter_data::{
    event::{DataKind, MarketEvent},
    streams::builder::dynamic::indexed::init_indexed_multi_exchange_market_stream,
    subscription::SubKind,
};
use barter_execution::{
    AccountEvent,
    order::id::StrategyId,
};
use barter_instrument::index::IndexedInstruments;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::{fs::File, io::BufReader, time::Duration};
use tracing::debug;

const FILE_PATH_SYSTEM_CONFIG: &str = "barter/examples/config/system_config.json";
const RISK_FREE_RETURN: Decimal = dec!(0.05);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialise Tracing
    init_logging();

    // Load SystemConfig
    let SystemConfig {
        instruments,
        executions,
    } = load_config()?;

    // Construct IndexedInstruments
    let instruments = IndexedInstruments::new(instruments);

    // Initialise MarketData Stream (including candles for ICT analysis)
    let market_stream = init_indexed_multi_exchange_market_stream(
        &instruments,
        &[SubKind::PublicTrades, SubKind::OrderBooksL1, SubKind::Candles],
    )
    .await?;

    // Create ICT strategy with custom configuration
    let ict_config = IctConfig {
        max_candle_history: 200,        // Keep more history for better analysis
        fvg_min_gap_pips: 5.0,         // Require larger gaps for FVG detection
        order_block_min_volume: 2000.0, // Higher volume threshold for order blocks
        risk_per_trade: dec!(0.02),     // 2% risk per trade
        max_position_size: dec!(0.1),   // 0.1 BTC max position
    };

    let ict_strategy = IctStrategy::new(
        StrategyId::new("ict_main_strategy"),
        ict_config,
    );

    // Construct System Args with ICT strategy
    let args = SystemArgs::new(
        &instruments,
        executions,
        LiveClock,
        ict_strategy,
        DefaultRiskManager::default(),
        market_stream,
        DefaultGlobalData::default(),
        |time_engine_start: DateTime<Utc>| IctInstrumentData::new(
            Default::default(),
            Default::default(),
        ),
    );

    // Build & run System with ICT strategy:
    let mut system = SystemBuilder::new(args)
        // Engine feed in Sync mode (Iterator input)
        .engine_feed_mode(EngineFeedMode::Iterator)
        // Audit feed is enabled (Engine sends audits)
        .audit_mode(AuditMode::Enabled)
        // Engine starts with TradingState::Disabled
        .trading_state(TradingState::Disabled)
        // Build System, but don't start spawning tasks yet
        .build::<EngineEvent, _>()?
        // Init System, spawning component tasks on the current runtime
        .init_with_runtime(tokio::runtime::Handle::current())
        .await?;

    // Take ownership of the Engine audit snapshot with updates
    let audit = system.audit.take().unwrap();

    // Run asynchronous AuditStream consumer
    let audit_task = tokio::spawn(async move {
        let mut audit_stream = audit.updates.into_stream();
        let mut ict_signals_detected = 0;
        
        while let Some(audit) = audit_stream.next().await {
            debug!(?audit, "AuditStream consumed AuditTick");
            
            // Monitor for ICT trading signals in the audit stream
            if let barter::engine::audit::EngineAudit::StrategyOrdersGenerated(orders) = &audit.event {
                if !orders.open.is_empty() {
                    ict_signals_detected += 1;
                    println!("🎯 ICT Strategy detected signal #{} - Generated {} order(s)", 
                             ict_signals_detected, orders.open.len());
                }
            }
            
            if audit.event.is_terminal() {
                break;
            }
        }
        
        println!("📊 ICT Strategy Session Summary:");
        println!("   • Total ICT signals detected: {}", ict_signals_detected);
        
        audit_stream
    });

    // Enable trading to start ICT strategy
    println!("🚀 Starting ICT Trading Strategy...");
    println!("   • Strategy: Inner Circle Trader (ICT)");
    println!("   • Features: Fair Value Gaps, Order Blocks, Market Structure");
    println!("   • Max Candle History: {}", 200);
    println!("   • FVG Min Gap: {} pips", 5.0);
    
    system.trading_state(TradingState::Enabled);

    // Let the ICT strategy run for a longer period to analyze market structure
    println!("⏳ Running ICT analysis for 30 seconds...");
    tokio::time::sleep(Duration::from_secs(30)).await;

    // Before shutting down, demonstrate position management
    println!("🛑 Stopping strategy - Cancelling orders and closing positions...");
    system.cancel_orders(InstrumentFilter::None);
    system.close_positions(InstrumentFilter::None);

    // Shutdown
    let (engine, _shutdown_audit) = system.shutdown().await?;
    let _audit_stream = audit_task.await?;

    // Generate TradingSummary
    let trading_summary = engine
        .trading_summary_generator(RISK_FREE_RETURN)
        .generate(Daily);

    // Print TradingSummary to terminal
    println!("\n📈 ICT Strategy Performance Summary:");
    trading_summary.print_summary();

    println!("\n✅ ICT Strategy Example completed successfully!");

    Ok(())
}

fn load_config() -> Result<SystemConfig, Box<dyn std::error::Error>> {
    let file = File::open(FILE_PATH_SYSTEM_CONFIG)?;
    let reader = BufReader::new(file);
    let config = serde_json::from_reader(reader)?;
    Ok(config)
}