// SPDX-License-Identifier: Apache-2.0

//! Deterministic manufacturing cost calculations.
//!
//! The crate is deliberately pure: it performs no network or filesystem I/O.
//! Callers provide frozen supplier offers, PCB specifications, dated fee
//! rules, and optional quotes. Replaying the same [`CostInput`] produces the
//! same [`CostReport`]. Money is accumulated as integer micro-units, never as
//! floating point totals.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use synth_supply::{PriceTier, PurchaseConstraints, StockStatus, SupplyStatus};

const MICROS_PER_UNIT: i64 = 1_000_000;

/// Integer-scaled money value. `micros` is one-millionth of a currency unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Money {
    pub currency: Currency,
    pub micros: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Currency {
    Usd,
    Eur,
    Gbp,
}

impl Money {
    #[allow(clippy::cast_precision_loss)]
    pub fn from_decimal(currency: Currency, value: f64) -> Result<Self, CostError> {
        if !value.is_finite() || value < 0.0 {
            return Err(CostError::InvalidMoney);
        }
        let micros = (value * MICROS_PER_UNIT as f64).round();
        if micros > i64::MAX as f64 {
            return Err(CostError::InvalidMoney);
        }
        Ok(Self {
            currency,
            micros: micros as i64,
        })
    }

    fn zero(currency: Currency) -> Self {
        Self {
            currency,
            micros: 0,
        }
    }

    fn checked_add(self, other: Self) -> Result<Self, CostError> {
        if self.currency != other.currency {
            return Err(CostError::CurrencyMismatch);
        }
        Ok(Self {
            currency: self.currency,
            micros: self
                .micros
                .checked_add(other.micros)
                .ok_or(CostError::Overflow)?,
        })
    }

    fn checked_mul(self, quantity: u64) -> Result<Self, CostError> {
        Ok(Self {
            currency: self.currency,
            micros: self
                .micros
                .checked_mul(i64::try_from(quantity).map_err(|_| CostError::Overflow)?)
                .ok_or(CostError::Overflow)?,
        })
    }
}

/// A frozen supplier offer used as a costing input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrozenOffer {
    pub status: SupplyStatus,
    /// RFC-3339 expiry for the offer, if the provider supplied one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartCostInput {
    pub sku: String,
    pub units_per_board: u32,
    pub allowance: u32,
    pub offer: FrozenOffer,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuantityScenario {
    pub fabricated_boards: u32,
    pub assembled_boards: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PcbSpecification {
    pub width_mm: f64,
    pub height_mm: f64,
    pub layers: u8,
    pub material: String,
    pub thickness_mm: f64,
    pub copper_oz: f64,
    pub finish: String,
    pub solder_mask: String,
    pub panel_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PcbEstimateProfile {
    pub name: String,
    pub currency: Currency,
    pub max_width_mm: f64,
    pub max_height_mm: f64,
    pub supported_layers: Vec<u8>,
    pub setup_fee: Money,
    pub per_board_fee: Money,
    pub per_cm2_fee: Money,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PcbQuote {
    pub quote_id: String,
    pub source: String,
    pub specification_hash: String,
    pub amount: Money,
    pub quoted_at: String,
    pub valid_until: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssemblyFeeRule {
    pub name: String,
    pub process: String,
    pub effective_from: String,
    pub expires_at: Option<String>,
    pub currency: Currency,
    pub setup_fee: Money,
    pub stencil_fee: Money,
    pub per_placement: Money,
    pub per_solder_joint: Money,
    pub manual_fee: Money,
    pub inspection_fee: Money,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssemblyInput {
    pub process: String,
    pub placements_per_board: u32,
    pub solder_joints_per_board: u32,
    pub manual_operations: u32,
    pub inspection_boards: u32,
    pub rule: AssemblyFeeRule,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeliveryInput {
    pub shipping: Option<Money>,
    pub duties: Option<Money>,
    pub taxes: Option<Money>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostInput {
    pub schema_version: String,
    pub snapshot_id: String,
    pub scenario: QuantityScenario,
    pub currency: Currency,
    pub parts: Vec<PartCostInput>,
    pub pcb_specification: Option<PcbSpecification>,
    /// Hash of the exact PCB specification used when importing a quote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pcb_specification_hash: Option<String>,
    pub pcb_profile: Option<PcbEstimateProfile>,
    pub pcb_quote: Option<PcbQuote>,
    pub assembly: Option<AssemblyInput>,
    pub delivery: Option<DeliveryInput>,
    pub budget: Option<Money>,
    /// RFC-3339 timestamp at which the report is evaluated.
    pub evaluated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostSource {
    FrozenOffer,
    Estimate,
    Quote,
    UserSupplied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostLineStatus {
    Known,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostLine {
    pub category: String,
    pub item: String,
    pub quantity: u64,
    pub amount: Option<Money>,
    pub source: CostSource,
    pub status: CostLineStatus,
    pub uncertain: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetOutcome {
    WithinBudget,
    OverBudget,
    Indeterminate,
    NoBudget,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostReport {
    pub schema_version: String,
    pub snapshot_id: String,
    pub scenario: QuantityScenario,
    pub currency: Currency,
    pub lines: Vec<CostLine>,
    pub known_subtotal: Money,
    pub complete_total: Option<Money>,
    pub budget: Option<Money>,
    pub budget_outcome: BudgetOutcome,
    pub has_expired_inputs: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuoteComparison {
    pub estimate: Money,
    pub quote: Money,
    pub difference: Money,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CostError {
    #[error("money value is negative, non-finite, or too large")]
    InvalidMoney,
    #[error("currency mismatch")]
    CurrencyMismatch,
    #[error("cost arithmetic overflow")]
    Overflow,
    #[error("fabricated quantity must be positive and assembled quantity cannot exceed it")]
    InvalidQuantity,
    #[error("conflicting frozen offers for purchasing SKU `{0}`")]
    ConflictingOffers(String),
    #[error("offer for `{0}` has no usable price tier")]
    MissingPrice(String),
    #[error("offer for `{0}` has an invalid price tier")]
    InvalidPriceTier(String),
    #[error("PCB specification is outside estimate profile `{0}`")]
    UnsupportedPcbProfile(String),
    #[error("PCB profile and quote cannot both be selected")]
    ConflictingPcbInputs,
    #[error("quote specification hash does not match the frozen PCB specification")]
    QuoteSpecificationMismatch,
    #[error("assembly fee rule does not match the requested process")]
    AssemblyProcessMismatch,
}

fn line_known(
    category: &str,
    item: &str,
    quantity: u64,
    amount: Money,
    source: CostSource,
) -> CostLine {
    CostLine {
        category: category.to_string(),
        item: item.to_string(),
        quantity,
        amount: Some(amount),
        source,
        status: CostLineStatus::Known,
        uncertain: false,
        reason: None,
        expires_at: None,
    }
}

fn line_unknown(
    category: &str,
    item: &str,
    quantity: u64,
    source: CostSource,
    reason: String,
) -> CostLine {
    CostLine {
        category: category.to_string(),
        item: item.to_string(),
        quantity,
        amount: None,
        source,
        status: CostLineStatus::Unknown,
        uncertain: true,
        reason: Some(reason),
        expires_at: None,
    }
}

fn rounded_purchase(required: u64, constraints: &PurchaseConstraints) -> Result<u64, CostError> {
    if required == 0 {
        return Err(CostError::InvalidQuantity);
    }
    let minimum = u64::from(constraints.moq.unwrap_or(0));
    let multiple = u64::from(constraints.order_multiple.unwrap_or(1));
    if multiple == 0 {
        return Err(CostError::InvalidQuantity);
    }
    let target = required.max(minimum);
    target
        .checked_add(multiple - 1)
        .map(|value| value / multiple * multiple)
        .ok_or(CostError::Overflow)
}

fn tier_price(tiers: &[PriceTier], quantity: u64) -> Option<&PriceTier> {
    tiers
        .iter()
        .filter(|tier| {
            let min = u64::from(tier.minimum_quantity);
            let max = tier.maximum_quantity.map(u64::from);
            min <= quantity && max.is_none_or(|max| quantity <= max)
        })
        .max_by_key(|tier| tier.minimum_quantity)
}

fn offer_line(
    input: &PartCostInput,
    assembled: u32,
    currency: Currency,
) -> Result<CostLine, CostError> {
    let required = u64::from(input.units_per_board)
        .checked_mul(u64::from(assembled))
        .and_then(|value| value.checked_add(u64::from(input.allowance)))
        .ok_or(CostError::Overflow)?;
    if required == 0 {
        return Err(CostError::InvalidQuantity);
    }
    let constraints = &input.offer.status.purchase;
    let purchase = rounded_purchase(required, constraints)?;
    let tier = tier_price(&constraints.price_tiers, purchase);
    let price = tier
        .map(|tier| tier.unit_price_usd)
        .or(input.offer.status.unit_price_usd);
    let Some(price) = price else {
        return Ok(line_unknown(
            "parts",
            &input.sku,
            purchase,
            CostSource::FrozenOffer,
            "provider supplied no usable price".into(),
        ));
    };
    // Frozen supplier prices in this contract are denominated in USD.
    if currency != Currency::Usd {
        return Err(CostError::CurrencyMismatch);
    }
    let unit = Money::from_decimal(currency, price)?;
    let mut line = line_known(
        "parts",
        &input.sku,
        purchase,
        unit.checked_mul(purchase)?,
        CostSource::FrozenOffer,
    );
    let mut uncertainty_reasons = Vec::new();
    if input.offer.status.stock_status != StockStatus::InStock
        || input.offer.status.stock_qty < purchase
    {
        uncertainty_reasons.push("supplier stock is unavailable or insufficient");
    }
    if input.offer.status.lifecycle.is_problematic() {
        uncertainty_reasons.push("supplier lifecycle is not active");
    }
    match input.offer.status.assembly_eligibility {
        synth_supply::AssemblyEligibility::Eligible => {}
        synth_supply::AssemblyEligibility::Ineligible => {
            uncertainty_reasons.push("supplier assembly service marks this part ineligible");
        }
        synth_supply::AssemblyEligibility::Unknown => {
            uncertainty_reasons.push("supplier assembly eligibility is unknown");
        }
    }
    line.uncertain = !uncertainty_reasons.is_empty();
    line.expires_at.clone_from(&input.offer.expires_at);
    if line.uncertain {
        line.reason = Some(uncertainty_reasons.join("; "));
    }
    Ok(line)
}

fn sum_known(lines: &[CostLine], currency: Currency) -> Result<(Money, bool), CostError> {
    let mut subtotal = Money::zero(currency);
    let mut complete = true;
    for line in lines {
        if let Some(amount) = line.amount {
            subtotal = subtotal.checked_add(amount)?;
        } else {
            complete = false;
        }
        if line.uncertain || line.status == CostLineStatus::Unknown {
            complete = false;
        }
    }
    Ok((subtotal, complete))
}

fn expired(now: &str, expires_at: Option<&str>) -> bool {
    expires_at.is_some_and(|expiry| expiry < now)
}

/// Calculate a deterministic report from frozen, offline inputs.
#[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
pub fn calculate(input: &CostInput) -> Result<CostReport, CostError> {
    if input.scenario.fabricated_boards == 0
        || input.scenario.assembled_boards > input.scenario.fabricated_boards
    {
        return Err(CostError::InvalidQuantity);
    }
    let mut lines = Vec::new();
    // Aggregate purchasing quantities before applying MOQ, multiples and tiers.
    let mut purchasing = std::collections::BTreeMap::<&str, PartCostInput>::new();
    if input.scenario.assembled_boards > 0 {
        for part in &input.parts {
            if let Some(existing) = purchasing.get_mut(part.sku.as_str()) {
                if existing.offer != part.offer {
                    return Err(CostError::ConflictingOffers(part.sku.clone()));
                }
                existing.units_per_board = existing
                    .units_per_board
                    .checked_add(part.units_per_board)
                    .ok_or(CostError::Overflow)?;
                existing.allowance = existing
                    .allowance
                    .checked_add(part.allowance)
                    .ok_or(CostError::Overflow)?;
            } else {
                purchasing.insert(&part.sku, part.clone());
            }
        }
        for part in purchasing.values() {
            lines.push(offer_line(
                part,
                input.scenario.assembled_boards,
                input.currency,
            )?);
        }
        if purchasing.is_empty() {
            lines.push(line_unknown(
                "parts",
                "assignments_required",
                0,
                CostSource::FrozenOffer,
                "no frozen purchasing assignments supplied".into(),
            ));
        }
    }

    if let Some(spec) = &input.pcb_specification {
        if input.pcb_profile.is_some() && input.pcb_quote.is_some() {
            return Err(CostError::ConflictingPcbInputs);
        }
        if let Some(quote) = &input.pcb_quote {
            if let Some(expected_hash) = &input.pcb_specification_hash
                && expected_hash != &quote.specification_hash
            {
                return Err(CostError::QuoteSpecificationMismatch);
            }
            let amount = quote.amount;
            if amount.currency != input.currency {
                return Err(CostError::CurrencyMismatch);
            }
            let mut line = line_known(
                "pcb",
                &quote.quote_id,
                u64::from(input.scenario.fabricated_boards),
                amount,
                CostSource::Quote,
            );
            line.expires_at = Some(quote.valid_until.clone());
            if input.pcb_specification_hash.is_none() {
                line.uncertain = true;
                line.reason =
                    Some("quote is not bound to the frozen PCB specification hash".into());
            }
            lines.push(line);
        } else if let Some(profile) = &input.pcb_profile {
            if profile.currency != input.currency
                || profile.setup_fee.currency != input.currency
                || profile.per_board_fee.currency != input.currency
                || profile.per_cm2_fee.currency != input.currency
            {
                return Err(CostError::CurrencyMismatch);
            }
            if !profile.supported_layers.contains(&spec.layers)
                || spec.width_mm <= 0.0
                || spec.height_mm <= 0.0
                || spec.width_mm > profile.max_width_mm
                || spec.height_mm > profile.max_height_mm
            {
                return Err(CostError::UnsupportedPcbProfile(profile.name.clone()));
            }
            let area_cm2 = spec.width_mm * spec.height_mm / 100.0;
            let area_fee = Money::from_decimal(
                profile.currency,
                area_cm2 * profile.per_cm2_fee.micros as f64 / MICROS_PER_UNIT as f64,
            )?;
            let board_fee = profile
                .per_board_fee
                .checked_mul(u64::from(input.scenario.fabricated_boards))?;
            let amount = profile
                .setup_fee
                .checked_add(area_fee.checked_mul(u64::from(input.scenario.fabricated_boards))?)?
                .checked_add(board_fee)?;
            lines.push(line_known(
                "pcb",
                &profile.name,
                u64::from(input.scenario.fabricated_boards),
                amount,
                CostSource::Estimate,
            ));
        } else {
            lines.push(line_unknown(
                "pcb",
                "quote_required",
                u64::from(input.scenario.fabricated_boards),
                CostSource::Estimate,
                "no supported estimate profile or supplier quote supplied".into(),
            ));
        }
    }

    if input.pcb_specification.is_none() {
        lines.push(line_unknown(
            "pcb",
            "specification_required",
            u64::from(input.scenario.fabricated_boards),
            CostSource::Estimate,
            "no PCB specification supplied".into(),
        ));
    }

    if let Some(assembly) = input
        .assembly
        .as_ref()
        .filter(|_| input.scenario.assembled_boards > 0)
    {
        if assembly.process != assembly.rule.process {
            return Err(CostError::AssemblyProcessMismatch);
        }
        if assembly.rule.currency != input.currency
            || assembly.rule.setup_fee.currency != input.currency
            || assembly.rule.stencil_fee.currency != input.currency
            || assembly.rule.per_placement.currency != input.currency
            || assembly.rule.per_solder_joint.currency != input.currency
            || assembly.rule.manual_fee.currency != input.currency
            || assembly.rule.inspection_fee.currency != input.currency
        {
            return Err(CostError::CurrencyMismatch);
        }
        let boards = u64::from(input.scenario.assembled_boards);
        let placements = u64::from(assembly.placements_per_board)
            .checked_mul(boards)
            .ok_or(CostError::Overflow)?;
        let joints = u64::from(assembly.solder_joints_per_board)
            .checked_mul(boards)
            .ok_or(CostError::Overflow)?;
        let manual = u64::from(assembly.manual_operations)
            .checked_mul(boards)
            .ok_or(CostError::Overflow)?;
        let inspection = u64::from(assembly.inspection_boards);
        let rule = &assembly.rule;
        let amount = rule
            .setup_fee
            .checked_add(rule.stencil_fee)?
            .checked_add(rule.per_placement.checked_mul(placements)?)?
            .checked_add(rule.per_solder_joint.checked_mul(joints)?)?
            .checked_add(rule.manual_fee.checked_mul(manual)?)?
            .checked_add(rule.inspection_fee.checked_mul(inspection)?)?;
        let mut line = line_known("assembly", &rule.name, boards, amount, CostSource::Estimate);
        line.expires_at.clone_from(&rule.expires_at);
        if rule.effective_from > input.evaluated_at {
            line.uncertain = true;
            line.reason = Some("assembly fee rule is not effective yet".into());
        }
        lines.push(line);
    }

    if input.assembly.is_none() && input.scenario.assembled_boards > 0 {
        lines.push(line_unknown(
            "assembly",
            "quote_required",
            u64::from(input.scenario.assembled_boards),
            CostSource::Estimate,
            "no assembly fee rule supplied".into(),
        ));
    }

    let unknown_delivery = DeliveryInput {
        shipping: None,
        duties: None,
        taxes: None,
    };
    {
        let delivery = input.delivery.as_ref().unwrap_or(&unknown_delivery);
        for (name, value) in [
            ("shipping", delivery.shipping),
            ("duties", delivery.duties),
            ("taxes", delivery.taxes),
        ] {
            if let Some(amount) = value {
                if amount.currency != input.currency {
                    return Err(CostError::CurrencyMismatch);
                }
                lines.push(line_known(
                    "delivery",
                    name,
                    1,
                    amount,
                    CostSource::UserSupplied,
                ));
            } else {
                lines.push(line_unknown(
                    "delivery",
                    name,
                    1,
                    CostSource::UserSupplied,
                    "not supplied".into(),
                ));
            }
        }
    }

    let has_expired_inputs = (input.scenario.assembled_boards > 0
        && input
            .parts
            .iter()
            .any(|part| expired(&input.evaluated_at, part.offer.expires_at.as_deref())))
        || input
            .pcb_quote
            .as_ref()
            .is_some_and(|quote| quote.valid_until.as_str() < input.evaluated_at.as_str())
        || (input.scenario.assembled_boards > 0
            && input
                .assembly
                .as_ref()
                .and_then(|assembly| assembly.rule.expires_at.as_deref())
                .is_some_and(|expiry| expiry < input.evaluated_at.as_str()));
    for line in &mut lines {
        if expired(&input.evaluated_at, line.expires_at.as_deref()) {
            line.uncertain = true;
            line.reason = Some("input has expired".into());
        }
    }
    let (known_subtotal, complete) = sum_known(&lines, input.currency)?;
    let complete_total = (complete && !has_expired_inputs).then_some(known_subtotal);
    let budget_outcome = match (input.budget, complete_total) {
        (None, _) => BudgetOutcome::NoBudget,
        (Some(_), None) => BudgetOutcome::Indeterminate,
        (Some(budget), Some(total)) if total.micros <= budget.micros => BudgetOutcome::WithinBudget,
        (Some(_), Some(_)) => BudgetOutcome::OverBudget,
    };
    Ok(CostReport {
        schema_version: "synth.cost-report.v1".into(),
        snapshot_id: input.snapshot_id.clone(),
        scenario: input.scenario.clone(),
        currency: input.currency,
        lines,
        known_subtotal,
        complete_total,
        budget: input.budget,
        budget_outcome,
        has_expired_inputs,
    })
}

/// Compare an estimate and a quote in the same currency.
pub fn compare_quote(estimate: Money, quote: Money) -> Result<QuoteComparison, CostError> {
    if estimate.currency != quote.currency {
        return Err(CostError::CurrencyMismatch);
    }
    Ok(QuoteComparison {
        estimate,
        quote,
        difference: Money {
            currency: estimate.currency,
            micros: quote
                .micros
                .checked_sub(estimate.micros)
                .ok_or(CostError::Overflow)?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_supply::{AssemblyEligibility, LifecycleStatus};

    fn offer(tiers: Vec<PriceTier>, stock: StockStatus) -> FrozenOffer {
        FrozenOffer {
            status: SupplyStatus {
                part_number: "C1".into(),
                distributor: "LCSC".into(),
                in_stock: stock == StockStatus::InStock,
                stock_qty: 100,
                stock_status: stock,
                assembly_eligibility: AssemblyEligibility::Eligible,
                moq: 1,
                purchase: PurchaseConstraints {
                    moq: Some(5),
                    order_multiple: Some(5),
                    price_tiers: tiers,
                },
                unit_price_usd: None,
                lifecycle: LifecycleStatus::Active,
                fetched_at: "2026-01-01T00:00:00Z".into(),
                snapshot_id: Some("snap-1".into()),
            },
            expires_at: Some("2027-01-01T00:00:00Z".into()),
        }
    }

    #[test]
    fn quantity_rounding_and_tier_boundary_are_exact() {
        let input = CostInput {
            schema_version: "test".into(),
            snapshot_id: "snap-1".into(),
            scenario: QuantityScenario {
                fabricated_boards: 5,
                assembled_boards: 3,
            },
            currency: Currency::Usd,
            parts: vec![PartCostInput {
                sku: "C1".into(),
                units_per_board: 2,
                allowance: 0,
                offer: offer(
                    vec![
                        PriceTier {
                            minimum_quantity: 1,
                            maximum_quantity: Some(9),
                            unit_price_usd: 0.10,
                        },
                        PriceTier {
                            minimum_quantity: 10,
                            maximum_quantity: None,
                            unit_price_usd: 0.08,
                        },
                    ],
                    StockStatus::InStock,
                ),
            }],
            pcb_specification: None,
            pcb_specification_hash: None,
            pcb_profile: None,
            pcb_quote: None,
            assembly: None,
            delivery: None,
            budget: None,
            evaluated_at: "2026-06-01T00:00:00Z".into(),
        };
        let report = calculate(&input).expect("cost report");
        assert_eq!(report.known_subtotal.micros, 800_000);
        assert!(report.complete_total.is_none());
        assert_eq!(report.lines[0].quantity, 10);
    }

    #[test]
    fn ineligible_assembly_is_explained_in_the_cost_line() {
        let mut input = incomplete_input();
        input.scenario = QuantityScenario {
            fabricated_boards: 1,
            assembled_boards: 1,
        };
        input.parts = vec![PartCostInput {
            sku: "C1".into(),
            units_per_board: 1,
            allowance: 0,
            offer: offer(
                vec![PriceTier {
                    minimum_quantity: 1,
                    maximum_quantity: None,
                    unit_price_usd: 0.10,
                }],
                StockStatus::InStock,
            ),
        }];
        input.parts[0].offer.status.assembly_eligibility = AssemblyEligibility::Ineligible;

        let report = calculate(&input).expect("cost report");
        assert!(report.lines[0].uncertain);
        assert_eq!(
            report.lines[0].reason.as_deref(),
            Some("supplier assembly service marks this part ineligible")
        );
    }

    #[test]
    fn unknown_price_keeps_total_incomplete_and_budget_indeterminate() {
        let mut input = CostInput {
            schema_version: "test".into(),
            snapshot_id: "snap-1".into(),
            scenario: QuantityScenario {
                fabricated_boards: 5,
                assembled_boards: 5,
            },
            currency: Currency::Usd,
            parts: vec![PartCostInput {
                sku: "C1".into(),
                units_per_board: 1,
                allowance: 0,
                offer: offer(vec![], StockStatus::Unknown),
            }],
            pcb_specification: None,
            pcb_specification_hash: None,
            pcb_profile: None,
            pcb_quote: None,
            assembly: None,
            delivery: None,
            budget: Some(Money::from_decimal(Currency::Usd, 10.0).unwrap()),
            evaluated_at: "2026-06-01T00:00:00Z".into(),
        };
        input.parts[0].offer.status.purchase.price_tiers.clear();
        let report = calculate(&input).expect("partial report");
        assert!(report.complete_total.is_none());
        assert_eq!(report.budget_outcome, BudgetOutcome::Indeterminate);
    }

    fn incomplete_input() -> CostInput {
        CostInput {
            schema_version: "synth.cost-input.v1".into(),
            snapshot_id: "test".into(),
            scenario: QuantityScenario {
                fabricated_boards: 5,
                assembled_boards: 5,
            },
            currency: Currency::Usd,
            parts: vec![],
            pcb_specification: None,
            pcb_specification_hash: None,
            pcb_profile: None,
            pcb_quote: None,
            assembly: None,
            delivery: None,
            budget: Some(Money::zero(Currency::Usd)),
            evaluated_at: "2026-06-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn omitted_categories_cannot_certify_a_zero_cost_build() {
        let report = calculate(&incomplete_input()).unwrap();
        assert!(report.complete_total.is_none());
        assert_eq!(report.budget_outcome, BudgetOutcome::Indeterminate);
        for category in ["parts", "pcb", "assembly", "delivery"] {
            assert!(report
                .lines
                .iter()
                .any(|line| line.category == category && line.status == CostLineStatus::Unknown));
        }
    }

    #[test]
    fn bare_boards_omit_parts_and_assembly_but_keep_unknown_delivery() {
        let mut input = incomplete_input();
        input.scenario.assembled_boards = 0;
        let report = calculate(&input).unwrap();
        assert!(
            report
                .lines
                .iter()
                .all(|line| line.category != "parts" && line.category != "assembly")
        );
        assert!(report.complete_total.is_none());
        input.scenario.assembled_boards = 6;
        assert_eq!(calculate(&input), Err(CostError::InvalidQuantity));
    }

    #[test]
    fn repeated_skus_select_price_tiers_after_aggregation() {
        let mut input = incomplete_input();
        let part = PartCostInput {
            sku: "C1".into(),
            units_per_board: 1,
            allowance: 0,
            offer: offer(
                vec![
                    PriceTier {
                        minimum_quantity: 1,
                        maximum_quantity: Some(9),
                        unit_price_usd: 0.10,
                    },
                    PriceTier {
                        minimum_quantity: 10,
                        maximum_quantity: None,
                        unit_price_usd: 0.08,
                    },
                ],
                StockStatus::InStock,
            ),
        };
        input.parts = vec![part.clone(), part];
        let report = calculate(&input).unwrap();
        assert_eq!(
            report
                .lines
                .iter()
                .filter(|line| line.category == "parts")
                .count(),
            1
        );
        assert_eq!(report.lines[0].quantity, 10);
        assert_eq!(report.known_subtotal.micros, 800_000);
        input.parts[1].offer.status.distributor = "Different supplier".into();
        assert_eq!(
            calculate(&input),
            Err(CostError::ConflictingOffers("C1".into()))
        );
    }

    #[test]
    fn bare_pcb_area_cost_scales_with_fabricated_quantity() {
        let mut input = incomplete_input();
        input.scenario.assembled_boards = 0;
        input.pcb_specification = Some(PcbSpecification {
            width_mm: 20.0,
            height_mm: 10.0,
            layers: 2,
            material: "FR4".into(),
            thickness_mm: 1.6,
            copper_oz: 1.0,
            finish: "HASL".into(),
            solder_mask: "green".into(),
            panel_count: 1,
        });
        let zero = Money::zero(Currency::Usd);
        input.pcb_profile = Some(PcbEstimateProfile {
            name: "test-only".into(),
            currency: Currency::Usd,
            max_width_mm: 100.0,
            max_height_mm: 100.0,
            supported_layers: vec![2],
            setup_fee: zero,
            per_board_fee: zero,
            per_cm2_fee: Money::from_decimal(Currency::Usd, 1.0).unwrap(),
        });
        input.delivery = Some(DeliveryInput {
            shipping: Some(zero),
            duties: Some(zero),
            taxes: Some(zero),
        });
        let report = calculate(&input).unwrap();
        assert_eq!(report.complete_total.unwrap().micros, 10_000_000);
        assert_eq!(report.budget_outcome, BudgetOutcome::OverBudget);
    }

    #[test]
    fn usd_offers_are_not_silently_relabelled_as_another_currency() {
        let mut input = incomplete_input();
        input.currency = Currency::Eur;
        input.parts.push(PartCostInput {
            sku: "C1".into(),
            units_per_board: 1,
            allowance: 0,
            offer: offer(
                vec![PriceTier {
                    minimum_quantity: 1,
                    maximum_quantity: None,
                    unit_price_usd: 1.0,
                }],
                StockStatus::InStock,
            ),
        });
        assert_eq!(calculate(&input), Err(CostError::CurrencyMismatch));
    }

    #[test]
    fn replay_is_stable_and_quote_difference_is_signed() {
        let money = Money::from_decimal(Currency::Usd, 1.25).unwrap();
        let comparison =
            compare_quote(money, Money::from_decimal(Currency::Usd, 1.50).unwrap()).unwrap();
        assert_eq!(comparison.difference.micros, 250_000);
        assert_eq!(
            serde_json::to_string(&money).unwrap(),
            serde_json::to_string(&money).unwrap()
        );
    }

    #[test]
    fn unbound_pcb_quote_is_visible_but_cannot_certify_total() {
        let mut input = incomplete_input();
        input.scenario.assembled_boards = 0;
        input.pcb_specification = Some(PcbSpecification {
            width_mm: 20.0,
            height_mm: 10.0,
            layers: 2,
            material: "FR4".into(),
            thickness_mm: 1.6,
            copper_oz: 1.0,
            finish: "HASL".into(),
            solder_mask: "green".into(),
            panel_count: 1,
        });
        input.pcb_quote = Some(PcbQuote {
            quote_id: "jlc-quote-1".into(),
            source: "JLCPCB".into(),
            specification_hash: "quote-hash".into(),
            amount: Money::from_decimal(Currency::Usd, 12.0).unwrap(),
            quoted_at: "2026-06-01T00:00:00Z".into(),
            valid_until: "2026-12-01T00:00:00Z".into(),
        });
        input.delivery = Some(DeliveryInput {
            shipping: Some(Money::zero(Currency::Usd)),
            duties: Some(Money::zero(Currency::Usd)),
            taxes: Some(Money::zero(Currency::Usd)),
        });

        let report = calculate(&input).unwrap();
        let pcb = report
            .lines
            .iter()
            .find(|line| line.category == "pcb")
            .unwrap();
        assert!(pcb.uncertain);
        assert_eq!(pcb.expires_at.as_deref(), Some("2026-12-01T00:00:00Z"));
        assert!(report.complete_total.is_none());
    }

    #[test]
    fn future_assembly_rule_is_not_used_as_a_certified_fee() {
        let mut input = incomplete_input();
        input.assembly = Some(AssemblyInput {
            process: "standard".into(),
            placements_per_board: 1,
            solder_joints_per_board: 2,
            manual_operations: 0,
            inspection_boards: 1,
            rule: AssemblyFeeRule {
                name: "future-rate-card".into(),
                process: "standard".into(),
                effective_from: "2027-01-01T00:00:00Z".into(),
                expires_at: None,
                currency: Currency::Usd,
                setup_fee: Money::zero(Currency::Usd),
                stencil_fee: Money::zero(Currency::Usd),
                per_placement: Money::zero(Currency::Usd),
                per_solder_joint: Money::zero(Currency::Usd),
                manual_fee: Money::zero(Currency::Usd),
                inspection_fee: Money::zero(Currency::Usd),
            },
        });
        let report = calculate(&input).unwrap();
        let assembly = report
            .lines
            .iter()
            .find(|line| line.category == "assembly")
            .unwrap();
        assert!(assembly.uncertain);
        assert_eq!(report.budget_outcome, BudgetOutcome::Indeterminate);
    }
}
