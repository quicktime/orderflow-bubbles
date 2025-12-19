# Fabio Valentini Nasdaq Orderflow Strategy — Complete Guide

## Who Is Fabio Valentini?

Italian professional scalper based in Dubai, consistently ranked in the **top 0.5% of CME Group futures traders**. Verified results from Robbins World Cup Trading Championships:
- 69% return (1st competition)
- 90% return (2nd competition)  
- **218% return** (3rd competition)
- 160% return (4th competition)

Over **2,000 trades in 12 months** with drawdowns under 20%. Co-created **DeepCharts** (rebranded Volumetrica platform).

---

## The Core Principle

> **"I don't try to catch the absolute low or high — I join the market at high-probability moments when participants have revealed themselves."**

The strategy exploits **trapped traders** at specific price levels where one side previously showed dominance, confirmed by real-time aggression.

---

## The Three-Element Framework

**ALL THREE must align before entry. No exceptions.**

| Element | Question | What You're Looking For |
|---------|----------|------------------------|
| **1. Market State** | Is the market balanced or imbalanced? | Balance = rotating around fair value (70% of time). Imbalance = directional push seeking new fair value |
| **2. Location** | Am I at a level where trapped traders will panic? | LVNs, POC, VWAP bands, prior day high/low, overnight high/low |
| **3. Aggression** | Are buyers/sellers ACTUALLY showing up right now? | Large bubbles, stacked imbalances, delta flip, absorption |

---

## Two Trading Models

### Model 1: Trend Model (Imbalance Continuation)
**When to use:** Market has broken structure and is trending  
**Best session:** New York open (9:30 AM - 12:00 PM ET)

**Execution:**
1. Identify the impulse leg that broke prior structure
2. Apply Volume Profile to that leg → find internal LVNs
3. Set alerts just before LVN levels (never blind limit orders)
4. When price pulls back to LVN, watch for aggression in trend direction
5. Enter with stop 1-2 ticks beyond the LVN
6. **Target:** Previous balance area POC

### Model 2: Mean Reversion Model (Failed Breakout Snap-back)
**When to use:** Breakout fails and price reclaims prior balance area  
**Best session:** London session or compressed market conditions

**Execution:**
1. Wait for clear reclaim back inside balance area
2. Wait for pullback into the reclaim leg
3. Apply Volume Profile to reclaim leg → find internal LVNs
4. On pullback to LVN, check for aggression in snap-back direction
5. Enter once price firmly returns inside balance
6. Stop just beyond the failed high/low
7. **Target:** Balance area POC

---

## Entry Triggers (What Must Appear)

### For Longs at predetermined levels:
- ✅ Large green bubbles/prints at support (Prism shows this)
- ✅ Buy imbalances on footprint at LVN
- ✅ CVD delta flip turning positive through zero
- ✅ Absorption: Heavy volume at level, price holds

### For Shorts (inverse):
- ✅ Large red bubbles/prints at resistance
- ✅ Sell imbalances on footprint at LVN
- ✅ CVD delta flip turning negative through zero
- ✅ Absorption: Heavy selling volume, price holds

### Critical Rule:
> **"No aggression = No trade"**
> Wait for buyers/sellers to reveal themselves — never anticipate.

---

## Stop Loss Placement

Stops are **structure-based**, not arbitrary tick counts:

1. Place stop **1-2 ticks beyond the aggressive print** that triggered entry
2. Add small buffer before obvious swing highs/lows
3. **If footprint loses pressure within 2-3 bars** → scratch to breakeven immediately

> **"If you're wrong, you should be wrong immediately."**

---

## Take Profit Targets

| Model | Target |
|-------|--------|
| Trend Model | Previous balance POC |
| Mean Reversion | Current balance area POC |

**Scaling:**
- First scale-out at **+1R** to remove emotional pressure
- Trail remaining position to last absorption or VWAP band after +1.5R

**Typical R-multiples:** 1:2.5 to 1:5

---

## Exit Signals (Abort Trade Regardless of P&L)

- ❌ Aggressive unwind with delta divergence + opposite stacked imbalances
- ❌ Clean VWAP reclaim against your bias
- ❌ Delta flip against your position
- ❌ Price reclaims invalidation level and holds for 2 bars

---

## Position Sizing: A/B/C System

| Grade | Description | Risk Allocation |
|-------|-------------|-----------------|
| **A Setup** | All 3 elements + multiple tape confirmations | Maximum (0.5-1% of account) |
| **B Setup** | Structure + 1 tape signal (imbalance OR delta, not both) | Half of maximum |
| **C Setup** | Structure only, tape pending — enter small, quick scratch if no confirm | Quarter of maximum |

**Dynamic risk principle:** Start each session at 0.25% risk per trade. Use early profits to fund larger positions. Worst losses come on small size; largest positions ride already-profitable days.

---

## Session Timing (All Times Eastern)

| Session | Trading Approach |
|---------|------------------|
| **NY Open (9:30 AM - 12:00 PM ET)** | PRIMARY focus — clearest directional bias |
| **Final Hour (3:00 - 4:00 PM ET)** | Secondary — only if structure intact |
| **London Session** | Mean reversion setups only — avoid breakouts (frequent fakeouts) |
| **Pre-market** | NO TRADING — wait for clear bias |

**Your Pacific Time Trading Window:** 6:30 AM - 9:00 AM PT (first 2.5 hours of RTH)

---

## No-Trade Conditions

Stop trading when you observe:
- ❌ Inside-day chop around prior Value Area
- ❌ Overlapping VWAPs (indecision)
- ❌ News-driven whips without absorption footprints
- ❌ Price ping-ponging mid-VWAP with no imbalance
- ❌ 2-5 minutes before/after Tier-1 economic releases

> **"If both sides are dead, simply don't force it."**

---

## Daily Loss Limits

- **Daily loss limit:** 2-3% of account maximum
- **Stop trading at 50-60% of daily limit** — elite traders exit early
- **Max consecutive losses:** 3 stop-outs → mandatory 30-minute reset
- **Loss-from-top:** After building profits, don't let day become significant loss

---

## Impulse Leg Identification (What to Profile)

### 5-Question Test:
1. Did it break prior swing high/low? (Yes = impulse)
2. Was it fast (3-5 candles max)? (Yes = impulse)
3. Candles mostly one color, little overlap? (Yes = impulse)
4. Volume increased on move? (Yes = impulse)
5. Move ≥30-50 points on NQ? (Yes = impulse)

**Scoring:** 4-5 yes = profile it | 2-3 yes = weak/skip | 0-1 yes = chop

### NQ Rules of Thumb:
| Move Size | Action |
|-----------|--------|
| 100+ points | Definitely profile (major impulse) |
| 50-100 points | Profile if fast and clean |
| 30-50 points | Only if very clean |
| <30 points | Skip — LVNs too tight |

> **"Obvious Test":** If you have to explain why it's an impulse, it's too small.

---

# YOUR EXACT TOOL SETUP

## Platform Stack

| Purpose | Tool | Cost |
|---------|------|------|
| **Charts + Analysis** | TradingView Premium | ~$60/month (you have this) |
| **Real-time Orderflow** | Prism | (your existing tool) |
| **Execution** | Tradovate (via prop firm) | Included |
| **Funding** | Prop Firm (Apex or MFF) | $77-167/month |

## TradingView Chart Layout (3 Charts)

### Chart 1: Left — 1-Minute Execution Chart
- Candlestick chart
- VWAP with bands (built-in)
- Prism on second monitor for bubble confirmation

### Chart 2: Top-Right — 1-Minute with Fixed Range Volume Profile
- Add "Fixed Range Volume Profile" drawing tool
- Use to profile impulse legs
- Mark LVNs manually with horizontal lines

### Chart 3: Bottom-Right — 15-Minute Structure
- Session Volume Profile (prior day)
- Auction Market Levels indicator (custom Pine Script)
- Shows: PDH, PDL, ONH, ONL

---

## TradingView Indicators to Add

### Built-in (Free):
1. **VWAP** — enable standard deviation bands (1σ, 2σ)
2. **Volume Profile Visible Range** — Row size: 100, Value Area: 70%
3. **Fixed Range Volume Profile** — drawing tool for impulse legs

### Custom Pine Scripts (Created for You):
1. **Auction Market Levels** — auto-plots PDH/PDL/ONH/ONL
2. **CVD with Divergence Detection** — shows delta divergence
3. **Market State Detector** — identifies balance vs imbalance

### Community (Search in Indicators):
- "Cumulative Volume Delta" by LuxAlgo (or similar)
- "Delta Volume" for candle-by-candle delta

---

## Pre-Session Checklist (Before 9:30 AM ET / 6:30 AM PT)

- [ ] Mark prior day High/Low/Close
- [ ] Mark overnight High/Low
- [ ] Identify balance vs imbalance (did price break structure overnight?)
- [ ] Apply Volume Profile to any overnight impulse legs → mark LVNs
- [ ] Set alerts 2-3 ticks before key levels
- [ ] Determine bias: bullish, bearish, or neutral (wait for clarity if neutral)

---

## Execution Workflow

```
1. TradingView alerts you: Price approaching your level
         ↓
2. Quick glance at Prism: Large bubbles appearing?
         ↓
   YES → Continue          NO → Wait/Skip
         ↓
3. Check CVD: Delta flipping in trade direction?
         ↓
   YES → Execute           NO → Wait for confirmation
         ↓
4. Place order in TradingView (connected to Tradovate)
         ↓
5. Manage trade: Stop 1-2 ticks beyond entry trigger
         ↓
6. Target: POC of prior/current balance area
```

---

# ACCOUNT SETUP STEPS

## Step 1: Free Practice (Before Paying)

1. Go to **tradovate.com** → Create free demo account
2. Connect Tradovate to TradingView:
   - TradingView → Trading Panel (bottom) → Connect Broker → Tradovate
3. Practice for 2-4 weeks with paper trading
4. Focus on identifying setups, not profits

## Step 2: Prop Firm Evaluation (When Ready)

**Recommended: Apex Trader Funding or My Funded Futures**

| Step | Action |
|------|--------|
| 1 | Sign up at apextraderfunding.com or myfundedfutures.com |
| 2 | Purchase evaluation (~$77-167, watch for 80% off promos) |
| 3 | Receive Tradovate credentials via email |
| 4 | Connect to TradingView (same process as demo) |
| 5 | Begin evaluation when ready (no time limit) |

---

## Contract Specifications

| Contract | Tick Value | Point Value | 10-Point Stop Cost |
|----------|-----------|-------------|-------------------|
| **NQ** (E-mini Nasdaq) | $5.00 | $20.00 | $200 |
| **MNQ** (Micro Nasdaq) | $0.50 | $2.00 | $20 |

**Ratio:** 1 NQ = 10 MNQ in exposure

**Start with MNQ** until consistently profitable.

---

# 30-DAY ONBOARDING PLAN

## Week 1: Setup & Study (No Trading)
- Day 1-2: Download TradeZella playbook (tradezella.com/playbooks/auction-market-playbook)
- Day 3-4: Set up TradingView with all indicators
- Day 5-7: Watch market during NY session, practice marking levels
- **Goal:** Identify balance vs imbalance correctly

## Week 2: Paper Trading
- Sign up for free Tradovate demo
- Trade 1 MNQ contract only
- Focus on identifying setups, not winning
- Mark every LVN on impulse legs
- **Goal:** Execute 20+ practice trades

## Week 3: Pattern Recognition
- Document every setup (screenshot + notes)
- Track: Which patterns worked? Which didn't?
- Which aggression signal works best for you?
- **Goal:** Identify your strongest setup type

## Week 4: System Refinement
- Review all trades from Week 3
- Consider starting prop firm evaluation
- Trade only A-setups initially
- **Goal:** Break-even or small profit

---

# KEY RESOURCES

## Free
- **TradeZella Playbook:** tradezella.com/playbooks/auction-market-playbook
- **YouTube:** "I Traded with the World #1 Scalper" (Andrea Cimitan channel)
- **LinkedIn:** linkedin.com/in/fabervaale (posts NQ model performance)

## Paid
- **World Class Edge:** worldclassedge.com (live trading floor, ~$200-500/month)
- **Morpheus Education:** morpheus.education (Italian-focused, has English content)

---

# QUICK REFERENCE CARD

## Entry Checklist:
✅ Market State identified (Balance/Imbalance)  
✅ At predetermined level (LVN, POC, PDH/PDL, ONH/ONL, VWAP band)  
✅ Aggression confirmed (bubbles, delta flip, stacked imbalances)  
✅ Stop placement defined BEFORE entry  
✅ Target identified (POC)

## Exit Rules:
- Stop: 1-2 ticks beyond entry trigger
- Target: Previous/current balance POC
- Scale: First partial at +1R
- Abort: Delta flip against, VWAP reclaim against, invalidation held 2 bars

## Position Size:
- A Setup: Full size (0.5-1% risk)
- B Setup: Half size
- C Setup: Quarter size (scratch if no confirm)

## Session Focus:
- Primary: 6:30 AM - 9:00 AM PT (NY open)
- Secondary: 12:00 PM - 1:00 PM PT (final hour)
- Avoid: Pre-market, mid-day chop, news releases

---

*Strategy based on Fabio Valentini's publicly available methodology from TradeZella playbook, YouTube interviews, and World Class Edge content.*
