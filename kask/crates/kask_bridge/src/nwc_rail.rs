//! Editor-side NIP-47 NWC implementation of [`hkask_wallet_core::LightningRail`] (D110 Phase 3).
//!
//! `wallet_nwc_uri` is read exclusively inside the editor process (`kask_bridge`)
//! and NEVER crosses into `hkask-mcp-wallet`. All `pay_invoice` calls run only
//! after mandatory native GPUI approval in `wallet_approval.rs` and verify
//! `SHA-256(preimage) == payment_hash` before reporting settlement.

use std::str::FromStr as _;
use std::time::Duration;

use async_trait::async_trait;
use hkask_types::wallet_wire::{WalletNwcOperationWire, WalletNwcResultWire};
use hkask_wallet_core::{
    CreatedLightningInvoice, LightningError, LightningInvoiceLookup, LightningInvoiceState,
    LightningPaymentResult, LightningRail, Millisatoshis, Satoshis,
    verify_preimage_matches_payment_hash,
};
use nwc::prelude::{
    LookupInvoiceRequest, MakeInvoiceRequest, NostrWalletConnect, NostrWalletConnectUri,
    PayInvoiceRequest, TransactionState,
};
use zeroize::Zeroizing;

/// Default per-call timeout for NIP-47 relay round-trips.
const NWC_CALL_TIMEOUT: Duration = Duration::from_secs(45);

/// NIP-47 NWC client implementing [`LightningRail`].
pub(crate) struct NwcRail {
    uri: Zeroizing<String>,
    timeout: Duration,
}

impl NwcRail {
    pub(crate) fn new(uri: Zeroizing<String>) -> Self {
        Self {
            uri,
            timeout: NWC_CALL_TIMEOUT,
        }
    }

    fn connect_client(&self) -> Result<NostrWalletConnect, LightningError> {
        let parsed = NostrWalletConnectUri::from_str(self.uri.trim()).map_err(|_| {
            LightningError::MalformedResponse(
                "stored NWC URI could not be parsed (re-enter in Settings > Kask > Wallet)"
                    .to_string(),
            )
        })?;
        Ok(NostrWalletConnect::new(parsed))
    }
}

fn map_nwc_error(err: nwc::error::Error) -> LightningError {
    let msg = err.to_string();
    if msg.to_ascii_lowercase().contains("timeout") {
        LightningError::Timeout
    } else if msg.to_ascii_lowercase().contains("insufficient")
        || msg.to_ascii_lowercase().contains("payment_failed")
        || msg.to_ascii_lowercase().contains("rejected")
    {
        LightningError::PaymentFailed(msg)
    } else {
        LightningError::Transport(msg)
    }
}

#[async_trait]
impl LightningRail for NwcRail {
    async fn get_balance_msat(&self) -> Result<Millisatoshis, LightningError> {
        let client = self.connect_client()?;
        let res = tokio::time::timeout(self.timeout, client.get_balance()).await;
        client.shutdown().await;
        match res {
            Ok(Ok(resp)) => Ok(Millisatoshis::new(resp.balance)),
            Ok(Err(err)) => Err(map_nwc_error(err)),
            Err(_) => Err(LightningError::Timeout),
        }
    }

    async fn make_invoice(
        &self,
        amount_msat: Millisatoshis,
        description: Option<String>,
        expiry_secs: Option<u32>,
    ) -> Result<CreatedLightningInvoice, LightningError> {
        let client = self.connect_client()?;
        let req = MakeInvoiceRequest {
            amount: amount_msat.as_u64(),
            description,
            description_hash: None,
            expiry: expiry_secs.map(u64::from),
        };
        let res = tokio::time::timeout(self.timeout, client.make_invoice(req)).await;
        client.shutdown().await;
        match res {
            Ok(Ok(resp)) => {
                let decoded = hkask_wallet_core::decode_bolt11(&resp.invoice).map_err(|_| {
                    LightningError::MalformedResponse(
                        "NWC make_invoice returned an unparseable BOLT11 invoice".to_string(),
                    )
                })?;
                let payment_hash = resp
                    .payment_hash
                    .filter(|h| !h.trim().is_empty())
                    .unwrap_or_else(|| decoded.payment_hash.clone());
                Ok(CreatedLightningInvoice {
                    bolt11: resp.invoice,
                    payment_hash,
                    amount_msat,
                    expires_at: decoded.expires_at_secs,
                })
            }
            Ok(Err(err)) => Err(map_nwc_error(err)),
            Err(_) => Err(LightningError::Timeout),
        }
    }

    async fn pay_invoice(
        &self,
        bolt11: &str,
        expected_payment_hash: &str,
    ) -> Result<LightningPaymentResult, LightningError> {
        let client = self.connect_client()?;
        let req = PayInvoiceRequest::new(bolt11.trim());
        let res = tokio::time::timeout(self.timeout, client.pay_invoice(req)).await;
        client.shutdown().await;
        match res {
            Ok(Ok(resp)) => {
                verify_preimage_matches_payment_hash(&resp.preimage, expected_payment_hash)?;
                Ok(LightningPaymentResult {
                    preimage: resp.preimage.trim().to_ascii_lowercase(),
                    payment_hash: expected_payment_hash.trim().to_ascii_lowercase(),
                    fee_paid_msat: Millisatoshis::new(resp.fees_paid.unwrap_or(0)),
                })
            }
            Ok(Err(err)) => Err(map_nwc_error(err)),
            Err(_) => Err(LightningError::Timeout),
        }
    }

    async fn lookup_invoice(
        &self,
        payment_hash: &str,
    ) -> Result<LightningInvoiceLookup, LightningError> {
        let client = self.connect_client()?;
        let req = LookupInvoiceRequest {
            payment_hash: Some(payment_hash.trim().to_ascii_lowercase()),
            invoice: None,
        };
        let res = tokio::time::timeout(self.timeout, client.lookup_invoice(req)).await;
        client.shutdown().await;
        match res {
            Ok(Ok(resp)) => {
                let state = match resp.state {
                    Some(TransactionState::Settled) => LightningInvoiceState::Settled,
                    Some(TransactionState::Pending | TransactionState::Accepted) => {
                        LightningInvoiceState::Pending
                    }
                    Some(TransactionState::Failed) => LightningInvoiceState::Failed,
                    Some(TransactionState::Expired) => LightningInvoiceState::Expired,
                    None => {
                        if resp
                            .preimage
                            .as_deref()
                            .is_some_and(|p| !p.trim().is_empty())
                        {
                            LightningInvoiceState::Settled
                        } else {
                            LightningInvoiceState::Pending
                        }
                    }
                };
                if state == LightningInvoiceState::Settled {
                    if let Some(preimage) = resp.preimage.as_deref() {
                        verify_preimage_matches_payment_hash(preimage, payment_hash)?;
                    } else {
                        return Err(LightningError::MalformedResponse(
                            "settled NWC lookup_invoice response omitted preimage".to_string(),
                        ));
                    }
                }
                Ok(LightningInvoiceLookup {
                    payment_hash: resp.payment_hash,
                    state,
                    preimage: resp.preimage,
                    fee_paid_msat: Some(Millisatoshis::new(resp.fees_paid)),
                })
            }
            Ok(Err(err)) => Err(map_nwc_error(err)),
            Err(_) => Err(LightningError::Timeout),
        }
    }
}

/// Dispatch a non-spending [`WalletNwcOperationWire`] over any [`LightningRail`].
pub(crate) async fn execute_nwc_query(
    rail: &dyn LightningRail,
    operation: &WalletNwcOperationWire,
) -> Result<WalletNwcResultWire, LightningError> {
    match operation {
        WalletNwcOperationWire::GetBalance => {
            let msat = rail.get_balance_msat().await?;
            let sats = msat.as_u64() / 1_000;
            Ok(WalletNwcResultWire::Balance { balance_sats: sats })
        }
        WalletNwcOperationWire::MakeInvoice {
            amount_sats,
            description,
        } => {
            let msat = Satoshis::new(*amount_sats)
                .checked_to_msat()
                .map_err(|e| LightningError::MalformedResponse(e.to_string()))?;
            let created = rail
                .make_invoice(msat, description.clone(), Some(3600))
                .await?;
            Ok(WalletNwcResultWire::Invoice {
                bolt11: created.bolt11,
                payment_hash: created.payment_hash,
                amount_sats: *amount_sats,
                expiry: created.expires_at,
            })
        }
        WalletNwcOperationWire::LookupInvoice { payment_hash } => {
            let lookup = rail.lookup_invoice(payment_hash).await?;
            let fee_paid_sats = lookup
                .fee_paid_msat
                .map(|m| m.to_satoshis_ceil().map(Satoshis::as_u64))
                .transpose()
                .map_err(|e| LightningError::MalformedResponse(e.to_string()))?;
            Ok(WalletNwcResultWire::Lookup {
                payment_hash: lookup.payment_hash,
                state: lookup.state.as_str().to_string(),
                preimage: lookup.preimage,
                fee_paid_sats,
            })
        }
    }
}
