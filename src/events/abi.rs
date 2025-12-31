//! ABI definitions for RexSwap events
//!
//! Note: RexSwap does NOT emit events for swaps/liquidity operations!
//! This file is kept for reference and for the few events that ARE emitted.

use alloy::sol;

// The only operational event RexSwap actually emits
sol! {
    /// Knockout cross event - emitted when a knockout pivot is crossed
    /// This is the ONLY swap-related event RexSwap emits!
    #[derive(Debug)]
    event RexKnockoutCross(
        bytes32 indexed pool,
        int24 indexed tick,
        bool isBid,
        uint32 pivotTime,
        uint64 feeMileage,
        uint160 commitEntropy
    );
}

// RexSwap governance/protocol events (from RexEvents.sol)
sol! {
    /// Authority transfer event
    #[derive(Debug)]
    event AuthorityTransfer(address indexed authority);

    /// New pool liquidity initialization value
    #[derive(Debug)]
    event SetNewPoolLiq(uint128 liq);

    /// Protocol take rate set
    #[derive(Debug)]
    event SetTakeRate(uint8 takeRate);

    /// Relayer take rate set
    #[derive(Debug)]
    event SetRelayerTakeRate(uint8 takeRate);

    /// Pool template disabled
    #[derive(Debug)]
    event DisablePoolTemplate(uint256 indexed poolIdx);

    /// Pool template set/updated
    #[derive(Debug)]
    event SetPoolTemplate(
        uint256 indexed poolIdx,
        uint16 feeRate,
        uint16 tickSize,
        uint8 jitThresh,
        uint8 knockout,
        address hooks
    );

    /// Take rate resync
    #[derive(Debug)]
    event ResyncTakeRate(
        address indexed base,
        address indexed quote,
        uint256 indexed poolIdx,
        uint8 takeRate
    );

    /// Price improvement threshold set
    #[derive(Debug)]
    event PriceImproveThresh(
        address indexed token,
        uint128 unitTickCollateral,
        uint16 awayTickTol
    );

    /// Treasury vault set
    #[derive(Debug)]
    event TreasurySet(
        address indexed treasury,
        uint64 indexed startTime
    );

    /// Protocol dividend collected
    #[derive(Debug)]
    event ProtocolDividend(
        address indexed token,
        address indexed recv
    );

    /// Proxy upgraded
    #[derive(Debug)]
    event UpgradeProxy(
        address indexed proxy,
        uint16 proxyIdx
    );

    /// Hot path open/close toggle
    #[derive(Debug)]
    event HotPathOpen(bool open);

    /// Safe mode toggle
    #[derive(Debug)]
    event SafeMode(bool enabled);
}

// RexPolicy governance events (from RexPolicy.sol)
sol! {
    /// Governance authority set
    #[derive(Debug)]
    event RexGovernAuthority(address ops, address treasury, address emergency);

    /// Ops resolution executed
    #[derive(Debug)]
    event RexResolutionOps(address minion, bytes cmd);

    /// Treasury resolution executed
    #[derive(Debug)]
    event RexResolutionTreasury(address minion, bool sudo, bytes cmd);

    /// Emergency halt triggered
    #[derive(Debug)]
    event RexEmergencyHalt(address minion, string reason);

    /// Policy rule set
    // Note: PolicyRule is a struct, simplified here
    #[derive(Debug)]
    event RexPolicySet(address conduit, uint16 proxyPath);

    /// Policy force applied
    #[derive(Debug)]
    event RexPolicyForce(address conduit, uint16 proxyPath);

    /// Emergency policy triggered
    #[derive(Debug)]
    event RexPolicyEmergency(address conduit, string reason);
}

// Deployer event
sol! {
    /// Contract deployed via RexDeployer
    #[derive(Debug)]
    event RexDeploy(address addr, uint256 salt);
}

/// Event topic signatures (keccak256 of event signature)
pub mod topics {
    use alloy::primitives::B256;
    use sha3::{Digest, Keccak256};

    fn compute_topic(signature: &str) -> B256 {
        let mut hasher = Keccak256::new();
        hasher.update(signature.as_bytes());
        B256::from_slice(&hasher.finalize())
    }

    lazy_static::lazy_static! {
        // The main operational event
        pub static ref REX_KNOCKOUT_CROSS: B256 = compute_topic(
            "RexKnockoutCross(bytes32,int24,bool,uint32,uint64,uint160)"
        );

        // Governance events
        pub static ref SET_POOL_TEMPLATE: B256 = compute_topic(
            "SetPoolTemplate(uint256,uint16,uint16,uint8,uint8,address)"
        );
        pub static ref AUTHORITY_TRANSFER: B256 = compute_topic(
            "AuthorityTransfer(address)"
        );
        pub static ref REX_GOVERN_AUTHORITY: B256 = compute_topic(
            "RexGovernAuthority(address,address,address)"
        );
        pub static ref REX_DEPLOY: B256 = compute_topic(
            "RexDeploy(address,uint256)"
        );
    }
}
