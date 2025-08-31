use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    program::invoke_signed,
    system_instruction,
};
use anchor_lang::solana_program::program_pack::Pack;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{
        self, Token, TokenAccount, InitializeMint2, MintTo, SetAuthority, spl_token::instruction::AuthorityType
    },
};



// -----------------------------------------------------------------------------
// ArenaForge MVP (Anchor)
// - Minimal viable on-chain core for: forge registry, arenas, battles,
//   lamport betting with rake, oracle-fed metrics, resolution and payouts,
//   and a reserve vault to fund winner rewards.
// - This version focuses on SOL (lamports) flows to keep the AMM/liquidity
//   integration pluggable. Extend with Token-2022 and Metaplex for mints/NFTs.
// -----------------------------------------------------------------------------

// Replace with your program id
declare_id!("3UL1eGG4TyyCVo4bVAMfCAoS5z99miKJZFiwt1X1CY2p");

// ----------------------------- Constants ------------------------------------
const GLOBAL_SEED: &[u8] = b"global";
const RESERVE_SEED: &[u8] = b"reserve";
const FORGE_SEED: &[u8] = b"forge";
const ARENA_SEED: &[u8] = b"arena";
const BATTLE_SEED: &[u8] = b"battle";
const BET_SEED: &[u8] = b"bet";


// ----------------------------- Accounts -------------------------------------


// ------------------------------ State ---------------------------------------

#[account]
pub struct Global {
    pub admin: Pubkey,
    pub fee_bps: u16,
    pub bet_rake_bps: u16,
    pub entry_fee_lamports: u64,
    pub bump: u8,
    pub reserve_bump: u8,
}
impl Global { pub const SIZE: usize = 32 + 2 + 2 + 8 + 1 + 1; }

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum ArenaKind { VolumeWar, SentimentDuel, Challenge }

#[account]
pub struct Arena {
    pub authority: Pubkey,
    pub kind: ArenaKind,
    pub reward_bps_from_reserve: u16,
    pub min_duration_secs: i64,
    pub oracle: Pubkey,
    pub bump: u8,
}
impl Arena { pub const SIZE: usize = 32 + 1 + 2 + 8 + 32 + 1; }

#[account]
pub struct Battle {
    pub arena: Pubkey,
    pub forge_a: Pubkey,
    pub forge_b: Pubkey,
    pub start_ts: i64,
    pub end_ts: i64,
    pub nonce: u64,
    pub metrics_a: u128,
    pub metrics_b: u128,
    pub pot_a: u64,
    pub pot_b: u64,
    pub resolved: bool,
    pub winner: u8, // 0 or 1
    pub bump: u8,
}
impl Battle {
    pub const SIZE: usize = 32 + 32 + 32 + 8 + 8 + 8 + 16 + 16 + 8 + 8 + 1 + 1 + 1;
}

#[account]
pub struct Bet {
    pub battle: Pubkey,
    pub bettor: Pubkey,
    pub side: u8,
    pub amount: u64,
    pub claimed: bool,
    pub bump: u8,
}
impl Bet { pub const SIZE: usize = 32 + 32 + 1 + 8 + 1 + 1; }


#[account]
pub struct Forge {
    pub mint: Pubkey,
    pub creator: Pubkey,
    pub bump: u8,
    pub created_at: i64,
}
impl Forge { pub const SIZE: usize = 32 + 32 + 1 + 8; }

#[derive(Accounts)]
#[instruction(seed: u64)]
pub struct ForgeNewToken<'info> {
    #[account(
        init,
        payer = creator,
        space = 8 + 32 + 32 + 1 + 8,
        seeds = [b"forge", creator.key().as_ref(), &seed.to_le_bytes()],
        bump
    )]
    pub forge: Account<'info, Forge>,

    /// Mint is a PDA: seeds ["mint", forge]
    #[account(
        mut,
        seeds = [b"mint", forge.key().as_ref()],
        bump
    )]
    pub mint: AccountInfo<'info>,

    /// PDA that acts as mint authority
    /// CHECK:
    #[account(
        seeds = [b"mint_auth", mint.key().as_ref()],
        bump
    )]
    pub mint_authority: UncheckedAccount<'info>,

    /// PDA as freeze authority (optional future use)
    /// CHECK:
    #[account(
        seeds = [b"freeze_auth", mint.key().as_ref()],
        bump
    )]
    pub freeze_authority: UncheckedAccount<'info>,

    #[account(mut)]
    pub creator: Signer<'info>,

    #[account(
        init_if_needed,
        payer = creator,
        associated_token::mint = mint,
        associated_token::authority = creator
    )]
    pub creator_ata: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[event]
pub struct ForgeCreated {
    pub mint: Pubkey,
    pub creator: Pubkey,
    pub ts: i64,
}

#[derive(Accounts)]
pub struct InitializeGlobal<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(
        init,
        payer = admin,
        space = 8 + Global::SIZE,
        seeds = [GLOBAL_SEED],
        bump,
    )]
    pub global: Account<'info, Global>,
    /// PDA that owns the reserve lamports
    #[account(
        seeds = [RESERVE_SEED],
        bump,
    )]
    /// CHECK: lamport vault (no data)
    pub reserve_vault: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RegisterForge<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// The wallet that will be recorded as project creator
    pub creator: Signer<'info>,
    /// Mint of the token (pre-created off-chain for MVP)
    /// CHECK: only used as Pubkey
    pub mint: AccountInfo<'info>,
    #[account(seeds = [GLOBAL_SEED], bump = global.bump)]
    pub global: Account<'info, Global>,
    #[account(seeds = [RESERVE_SEED], bump = global.reserve_bump)]
    /// CHECK: lamport vault
    pub reserve_vault: AccountInfo<'info>,
    #[account(
        init,
        payer = payer,
        space = 8 + Forge::SIZE,
        seeds = [FORGE_SEED, mint.key().as_ref()],
        bump,
    )]
    pub forge: Account<'info, Forge>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CreateArenaCtx<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(seeds = [GLOBAL_SEED], bump = global.bump)]
    pub global: Account<'info, Global>,
    /// CHECK: oracle signer key recorded on the arena
    pub oracle: UncheckedAccount<'info>,
    #[account(
        init,
        payer = authority,
        space = 8 + Arena::SIZE,
        seeds = [ARENA_SEED, authority.key().as_ref()],
        bump,
    )]
    pub arena: Account<'info, Arena>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(nonce: u64)]
pub struct InitBattle<'info> {
    #[account(mut)]
    pub authority: Signer<'info>, // must match arena.authority
    #[account(seeds = [GLOBAL_SEED], bump = global.bump)]
    pub global: Account<'info, Global>,
    #[account(seeds = [RESERVE_SEED], bump = global.reserve_bump)]
    /// CHECK: lamport vault
    pub reserve_vault: AccountInfo<'info>,

    #[account(has_one = authority)]
    pub arena: Account<'info, Arena>,

    /// Both forges being matched
    pub forge_a: Account<'info, Forge>,
    pub forge_b: Account<'info, Forge>,

    /// Signers must be the creators of each forge to pay entry fee
    #[account(mut, address = forge_a.creator)]
    pub forge_a_creator: Signer<'info>,
    #[account(mut, address = forge_b.creator)]
    pub forge_b_creator: Signer<'info>,

    #[account(
        init,
        payer = authority,
        space = 8 + Battle::SIZE,
        seeds = [
            BATTLE_SEED,
            arena.key().as_ref(),
            forge_a.key().as_ref(),
            forge_b.key().as_ref(),
            &nonce.to_le_bytes()
        ],
        bump,
    )]
    pub battle: Account<'info, Battle>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateMetrics<'info> {
    /// CHECK: oracle authority account must match arena.oracle
    pub oracle: Signer<'info>,
    pub arena: Account<'info, Arena>,
    #[account(mut, has_one = arena)]
    pub battle: Account<'info, Battle>,
}

#[derive(Accounts)]
pub struct PlaceBet<'info> {
    #[account(mut)]
    pub bettor: Signer<'info>,
    pub arena: Account<'info, Arena>,
    pub forge_a: Account<'info, Forge>,
    pub forge_b: Account<'info, Forge>,
    #[account(mut, has_one = arena, has_one = forge_a, has_one = forge_b)]
    pub battle: Account<'info, Battle>,
    #[account(
        init_if_needed,
        payer = bettor,
        space = 8 + Bet::SIZE,
        seeds = [BET_SEED, battle.key().as_ref(), bettor.key().as_ref()],
        bump,
    )]
    pub bet: Account<'info, Bet>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ResolveBattle<'info> {
    pub authority: Signer<'info>, // must match arena.authority
    #[account(seeds = [GLOBAL_SEED], bump = global.bump)]
    pub global: Account<'info, Global>,
    #[account(mut, seeds = [RESERVE_SEED], bump = global.reserve_bump)]
    /// CHECK: lamport vault
    pub reserve_vault: AccountInfo<'info>,
    #[account(has_one = authority)]
    pub arena: Account<'info, Arena>,
    #[account(mut, has_one = arena)]
    pub battle: Account<'info, Battle>,
    /// CHECK: recipient of reserve reward (e.g., winner liquidity manager or creator)
    #[account(mut)]
    pub winner_recipient: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ClaimBet<'info> {
    #[account(mut)]
    pub bettor: Signer<'info>,
    pub arena: Account<'info, Arena>,
    pub forge_a: Account<'info, Forge>,
    pub forge_b: Account<'info, Forge>,
    #[account(mut, has_one = arena, has_one = forge_a, has_one = forge_b)]
    pub battle: Account<'info, Battle>,
    #[account(mut, seeds = [BET_SEED, battle.key().as_ref(), bettor.key().as_ref()], bump = bet.bump)]
    pub bet: Account<'info, Bet>,
    #[account(seeds = [GLOBAL_SEED], bump = global.bump)]
    pub global: Account<'info, Global>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SweepRake<'info> {
    pub admin: Signer<'info>,
    #[account(seeds = [GLOBAL_SEED], bump = global.bump, has_one = admin)]
    pub global: Account<'info, Global>,
    #[account(seeds = [RESERVE_SEED], bump = global.reserve_bump)]
    /// CHECK: lamport vault
    pub reserve_vault: AccountInfo<'info>,
    pub arena: Account<'info, Arena>,
    pub forge_a: Account<'info, Forge>,
    pub forge_b: Account<'info, Forge>,
    #[account(mut, has_one = arena, has_one = forge_a, has_one = forge_b)]
    pub battle: Account<'info, Battle>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DepositToReserve<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(seeds = [GLOBAL_SEED], bump = global.bump)]
    pub global: Account<'info, Global>,
    #[account(mut, seeds = [RESERVE_SEED], bump = global.reserve_bump)]
    /// CHECK: lamport vault
    pub reserve_vault: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct WithdrawFromReserve<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(seeds = [GLOBAL_SEED], bump = global.bump)]
    pub global: Account<'info, Global>,
    #[account(mut, seeds = [RESERVE_SEED], bump = global.reserve_bump)]
    /// CHECK: lamport vault
    pub reserve_vault: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

// ------------------------------ Utils ---------------------------------------
// fn transfer_lamports<'a>(from: &Signer<'a>, to: &AccountInfo<'a>, amount: u64) -> Result<()> {
//     require!(**from.to_account_info().lamports.borrow() >= amount, ArenaErr::Insufficient);
//     let ix = system_instruction::transfer(from.key, to.key, amount);
//     anchor_lang::solana_program::program::invoke(
//         &ix,
//         &[from.to_account_info(), to.clone()],
//     )?;
//     Ok(())
// }

fn transfer_lamports<'info>(
    from: &Signer<'info>,
    to: &AccountInfo<'info>,
    amount: u64,
) -> Result<()> {
    require!(**from.to_account_info().lamports.borrow() >= amount, ArenaErr::Insufficient);
    let ix = system_instruction::transfer(from.key, to.key, amount);
    anchor_lang::solana_program::program::invoke(&ix, &[from.to_account_info(), to.clone()])?;
    Ok(())
}

fn transfer_lamports_signed<'info>(
    from: &AccountInfo<'info>,
    to: &AccountInfo<'info>,
    amount: u64,
    signer_seeds: &[&[u8]],
) -> Result<()> {
    require!(**from.lamports.borrow() >= amount, ArenaErr::Insufficient);
    let ix = system_instruction::transfer(from.key, to.key, amount);
    invoke_signed(&ix, &[from.clone(), to.clone()], &[signer_seeds])?;
    Ok(())
}

// ------------------------------ Errors --------------------------------------
#[error_code]
pub enum ArenaErr {
    #[msg("invalid basis points")] InvalidBps,
    #[msg("missing PDA bump")] MissingBump,
    #[msg("duration too short")] DurationTooShort,
    #[msg("bad start/end times")] BadTimes,
    #[msg("unauthorized")] Unauthorized,
    #[msg("round not active")] NotInRound,
    #[msg("round already ended")] RoundEnded,
    #[msg("battle not ended")] NotEnded,
    #[msg("battle not resolved")] NotResolved,
    #[msg("already resolved")] AlreadyResolved,
    #[msg("bad side")] BadSide,
    #[msg("zero amount")] ZeroAmount,
    #[msg("overflow") ] Overflow,
    #[msg("insufficient funds")] Insufficient,
    #[msg("empty winner pot")] EmptyWinnerPot,
    #[msg("bad bet link")] BadBet,
    #[msg("already claimed")] AlreadyClaimed,
}

#[program]
pub mod arenaforge_mvp {
    use super::*;

    // ------------------------- Bootstrap ------------------------------------
    pub fn initialize_global(
        ctx: Context<InitializeGlobal>,
        fee_bps: u16,           // platform fee on forges/entries (<= 10000)
        bet_rake_bps: u16,      // rake on betting pools at resolution
        entry_fee_lamports: u64,
    ) -> Result<()> {
        require!(fee_bps <= 10_000, ArenaErr::InvalidBps);
        require!(bet_rake_bps <= 10_000, ArenaErr::InvalidBps);

        let global = &mut ctx.accounts.global;
        global.admin = ctx.accounts.admin.key();
        global.fee_bps = fee_bps;
        global.bet_rake_bps = bet_rake_bps;
        global.entry_fee_lamports = entry_fee_lamports;
    global.bump = ctx.bumps.global;
    global.reserve_bump = ctx.bumps.reserve_vault;
        Ok(())
    }

    // ------------------------- Forge Registry --------------------------------
    /// Registers a token forge (project). For MVP, the mint is pre-created off-chain.
    /// A flat fee goes to the reserve vault.
    pub fn register_forge(ctx: Context<RegisterForge>) -> Result<()> {
        let global = &ctx.accounts.global;
        // Collect flat forge fee into reserve
        transfer_lamports(
            &ctx.accounts.payer,
            &ctx.accounts.reserve_vault,
            global.entry_fee_lamports,
        )?;

        let forge = &mut ctx.accounts.forge;
        forge.creator = ctx.accounts.creator.key();
        forge.mint = ctx.accounts.mint.key();
        forge.created_at = Clock::get()?.unix_timestamp;
    forge.bump = ctx.bumps.forge;
        Ok(())
    }

    // ------------------------- Arenas ----------------------------------------
    pub fn create_arena(
    ctx: Context<CreateArenaCtx>,
    kind: ArenaKind,
    reward_bps_from_reserve: u16,
    min_duration_secs: i64,
) -> Result<()> {
    require!(reward_bps_from_reserve <= 10_000, ArenaErr::InvalidBps);
    require!(min_duration_secs >= 60, ArenaErr::DurationTooShort);

    let arena = &mut ctx.accounts.arena;
    arena.authority = ctx.accounts.authority.key();
    arena.kind = kind;
    arena.reward_bps_from_reserve = reward_bps_from_reserve;
    arena.min_duration_secs = min_duration_secs;
    arena.oracle = ctx.accounts.oracle.key();
    arena.bump = ctx.bumps.arena;   // ← still fine
    Ok(())
}

    // Create a new battle under an arena. Admin/arena authority seeded; creators pay entry fee.
    pub fn init_battle(
        ctx: Context<InitBattle>,
        nonce: u64,          // allows multiple battles per pair
        start_ts: i64,
        end_ts: i64,
    ) -> Result<()> {
        let arena = &ctx.accounts.arena;
        require!(end_ts > start_ts, ArenaErr::BadTimes);
        require!(end_ts - start_ts >= arena.min_duration_secs, ArenaErr::DurationTooShort);

        // Collect entry fees from both token creators into reserve
        let global = &ctx.accounts.global;
        transfer_lamports(
            &ctx.accounts.forge_a_creator,
            &ctx.accounts.reserve_vault,
            global.entry_fee_lamports,
        )?;
        transfer_lamports(
            &ctx.accounts.forge_b_creator,
            &ctx.accounts.reserve_vault,
            global.entry_fee_lamports,
        )?;

        let battle = &mut ctx.accounts.battle;
        battle.arena = arena.key();
        battle.forge_a = ctx.accounts.forge_a.key();
        battle.forge_b = ctx.accounts.forge_b.key();
        battle.start_ts = start_ts;
        battle.end_ts = end_ts;
        battle.nonce = nonce;
        battle.metrics_a = 0;
        battle.metrics_b = 0;
        battle.pot_a = 0;
        battle.pot_b = 0;
        battle.resolved = false;
        battle.winner = 255; // unknown
    battle.bump = ctx.bumps.battle;
        Ok(())
    }

    // Oracle-authorized metric updates (e.g., aggregated volume/sentiment scores)
    pub fn update_metrics(ctx: Context<UpdateMetrics>, add_a: u128, add_b: u128) -> Result<()> {
        let arena = &ctx.accounts.arena;
        require_keys_eq!(arena.oracle, ctx.accounts.oracle.key(), ArenaErr::Unauthorized);
        let battle = &mut ctx.accounts.battle;
        require!(!battle.resolved, ArenaErr::AlreadyResolved);
        let now = Clock::get()?.unix_timestamp;
        require!(now >= battle.start_ts && now <= battle.end_ts, ArenaErr::NotInRound);
        battle.metrics_a = battle
            .metrics_a
            .checked_add(add_a)
            .ok_or(ArenaErr::Overflow)?;
        battle.metrics_b = battle
            .metrics_b
            .checked_add(add_b)
            .ok_or(ArenaErr::Overflow)?;
        Ok(())
    }

    // Place a bet on side 0 (A) or 1 (B). Lamports are escrowed in the Battle PDA.
    pub fn place_bet(ctx: Context<PlaceBet>, side: u8, amount_lamports: u64) -> Result<()> {
    require!(side == 0 || side == 1, ArenaErr::BadSide);
    require!(amount_lamports > 0, ArenaErr::ZeroAmount);

    let now = Clock::get()?.unix_timestamp;
    require!(now < ctx.accounts.battle.end_ts, ArenaErr::RoundEnded);

    // 1) escrow first (no mutable borrow yet)
    transfer_lamports(
        &ctx.accounts.bettor,
        &ctx.accounts.battle.to_account_info(),
        amount_lamports,
    )?;

    // 2) now update pots with a mutable borrow
    let battle = &mut ctx.accounts.battle;
    if side == 0 {
        battle.pot_a = battle.pot_a.checked_add(amount_lamports).ok_or(ArenaErr::Overflow)?;
    } else {
        battle.pot_b = battle.pot_b.checked_add(amount_lamports).ok_or(ArenaErr::Overflow)?;
    }

    // 3) upsert bet
    let bet = &mut ctx.accounts.bet;
    if bet.amount == 0 {
        bet.battle  = battle.key();
        bet.bettor  = ctx.accounts.bettor.key();
        bet.side    = side;
        bet.claimed = false;
        bet.bump    = ctx.bumps.bet;
    }
    bet.amount = bet.amount.checked_add(amount_lamports).ok_or(ArenaErr::Overflow)?;
    Ok(())
}

    // Resolve the battle, decide the winner, send reserve reward to winner recipient, and
    // lock in pots for later user claims.
    pub fn resolve_battle(ctx: Context<ResolveBattle>) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let battle = &mut ctx.accounts.battle;
        require!(now >= battle.end_ts, ArenaErr::NotEnded);
        require!(!battle.resolved, ArenaErr::AlreadyResolved);

        // Decide winner by metrics; tie-break by pot sizes
        let winner = if battle.metrics_a > battle.metrics_b {
            0
        } else if battle.metrics_b > battle.metrics_a {
            1
        } else if battle.pot_a >= battle.pot_b {
            0
        } else {
            1
        };
        battle.winner = winner;
        battle.resolved = true;

        // Pay reserve reward to provided recipient
        let arena = &ctx.accounts.arena;
        let global = &ctx.accounts.global;

        // compute reward = min(reserve_balance, bps of reserve_balance)
        let reserve_balance = **ctx.accounts.reserve_vault.to_account_info().lamports.borrow();
        // Use a conservative cap: pay up to 5% of current reserve if arena.reward_bps_from_reserve == 500
        let reward = reserve_balance
            .checked_mul(arena.reward_bps_from_reserve as u64)
            .ok_or(ArenaErr::Overflow)?
            .checked_div(10_000)
            .ok_or(ArenaErr::Overflow)?;

        if reward > 0 {
            let seeds = &[GLOBAL_SEED, &[global.bump]];
            let reserve_seeds = &[RESERVE_SEED, &[global.reserve_bump]];

            // transfer lamports from reserve to winner recipient
            invoke_signed(
                &system_instruction::transfer(
                    ctx.accounts.reserve_vault.key,
                    ctx.accounts.winner_recipient.key,
                    reward,
                ),
                &[
                    ctx.accounts.reserve_vault.to_account_info(),
                    ctx.accounts.winner_recipient.to_account_info(),
                    ctx.accounts.system_program.to_account_info(),
                ],
                &[reserve_seeds],
            )?;
        }
        Ok(())
    }

    // After resolution, winners call claim_bet to receive their share of the pot minus rake.
    pub fn claim_bet(ctx: Context<ClaimBet>) -> Result<()> {
        let global = &ctx.accounts.global;
        // Use immutable borrow for transfer first
        let payout;
        {
            let battle = &ctx.accounts.battle;
            let bet = &ctx.accounts.bet;
            require!(battle.resolved, ArenaErr::NotResolved);
            require!(!bet.claimed, ArenaErr::AlreadyClaimed);
            require_keys_eq!(bet.battle, battle.key(), ArenaErr::BadBet);
            require_keys_eq!(bet.bettor, ctx.accounts.bettor.key(), ArenaErr::Unauthorized);
            let (winner_pot, loser_pot) = if battle.winner == 0 {
                (battle.pot_a, battle.pot_b)
            } else {
                (battle.pot_b, battle.pot_a)
            };
            // If user bet the losing side, payout is 0.
            if (battle.winner == 0 && bet.side == 1) || (battle.winner == 1 && bet.side == 0) {
                let mut bet = ctx.accounts.bet.clone();
                bet.claimed = true;
                return Ok(());
            }
            let total_pool = winner_pot.checked_add(loser_pot).ok_or(ArenaErr::Overflow)?;
            let rake = total_pool.checked_mul(global.bet_rake_bps as u64).ok_or(ArenaErr::Overflow)?.checked_div(10_000).ok_or(ArenaErr::Overflow)?;
            let distributable = total_pool.checked_sub(rake).ok_or(ArenaErr::Overflow)?;
            require!(winner_pot > 0, ArenaErr::EmptyWinnerPot);
            payout = ((bet.amount as u128)
                .checked_mul(distributable as u128)
                .ok_or(ArenaErr::Overflow)?
                .checked_div(winner_pot as u128)
                .ok_or(ArenaErr::Overflow)?) as u64;
        }
        let arena_key   = ctx.accounts.arena.key();
        let forge_a_key = ctx.accounts.forge_a.key();
        let forge_b_key = ctx.accounts.forge_b.key();

        let battle_signer_seeds: &[&[u8]] = &[
            BATTLE_SEED,
            arena_key.as_ref(),
            forge_a_key.as_ref(),
            forge_b_key.as_ref(),
            &ctx.accounts.battle.nonce.to_le_bytes(),
            &[ctx.accounts.battle.bump],
        ];

        transfer_lamports_signed(
            &ctx.accounts.battle.to_account_info(),
            &ctx.accounts.bettor.to_account_info(),
            payout,
            battle_signer_seeds,
        )?;

        let bet = &mut ctx.accounts.bet;
        bet.claimed = true;
        Ok(())
    }

    // Admin sweep: move accumulated rakes from battle PDA to reserve vault after resolution.
    pub fn sweep_rake(ctx: Context<SweepRake>) -> Result<()> {
        let global = &ctx.accounts.global;
        let battle = &ctx.accounts.battle;
        require!(battle.resolved, ArenaErr::NotResolved);

        let (winner_pot, loser_pot) = if battle.winner == 0 {
            (battle.pot_a, battle.pot_b)
        } else {
            (battle.pot_b, battle.pot_a)
        };
        let total_pool = winner_pot.checked_add(loser_pot).ok_or(ArenaErr::Overflow)?;
        let rake = total_pool
            .checked_mul(global.bet_rake_bps as u64)
            .ok_or(ArenaErr::Overflow)?
            .checked_div(10_000)
            .ok_or(ArenaErr::Overflow)?;

        // Estimate remaining lamports in battle = total_pool - sum(claimed payouts)
        // For MVP we transfer up to 'rake' or available balance, whichever is smaller.
        let available = **ctx.accounts.battle.to_account_info().lamports.borrow();
        let to_transfer = core::cmp::min(available, rake);
        if to_transfer > 0 {

        let arena_key   = ctx.accounts.arena.key();
        let forge_a_key = ctx.accounts.forge_a.key();
        let forge_b_key = ctx.accounts.forge_b.key();

        let battle_signer_seeds: &[&[u8]] = &[
            BATTLE_SEED,
            arena_key.as_ref(),
            forge_a_key.as_ref(),
            forge_b_key.as_ref(),
            &ctx.accounts.battle.nonce.to_le_bytes(),
            &[ctx.accounts.battle.bump],
        ];

        transfer_lamports_signed(
            &ctx.accounts.battle.to_account_info(),
            &ctx.accounts.reserve_vault,
            to_transfer,
            battle_signer_seeds,
            )?;
        }
        Ok(())
    }

    // Fund reserve vault (anyone can add liquidity for rewards)
    pub fn deposit_to_reserve(_ctx: Context<DepositToReserve>, _amount: u64) -> Result<()> {
        // no-op: transfer handled in CPI by payer in accounts constraints
        Ok(())
    }

    // Admin withdrawal from reserve
    pub fn withdraw_from_reserve(ctx: Context<WithdrawFromReserve>, amount: u64) -> Result<()> {
        require_keys_eq!(ctx.accounts.global.admin, ctx.accounts.admin.key(), ArenaErr::Unauthorized);
        transfer_lamports_signed(
            &ctx.accounts.reserve_vault,
            &ctx.accounts.admin.to_account_info(),
            amount,
            &[RESERVE_SEED, &[ctx.accounts.global.reserve_bump]],
        )
    }

    pub fn forge_new_token(
        ctx: Context<ForgeNewToken>,
        seed: u64,           // lets a creator make many tokens; ties PDAs
        decimals: u8,
        initial_supply: u64, // raw base units (respect `decimals`)
        lock_mint: bool,
    ) -> Result<()> {
        // record
        let forge = &mut ctx.accounts.forge;
        forge.mint = ctx.accounts.mint.key();
        forge.creator = ctx.accounts.creator.key();
    forge.bump = ctx.bumps.forge;
        forge.created_at = Clock::get()?.unix_timestamp;


        // ----- create mint account at PDA (owner = Token program) -----
     let mint_len: usize = anchor_spl::token::spl_token::state::Mint::LEN;
    let lamports: u64 = Rent::get()?.minimum_balance(mint_len);

        let seeds_mint: &[&[u8]] = &[
            b"mint",
            &forge.key().to_bytes(),
            &[ctx.bumps.mint],
        ];

        invoke_signed(
            &system_instruction::create_account(
                &ctx.accounts.creator.key(),
                &ctx.accounts.mint.key(),
                lamports,
                mint_len as u64,
                &ctx.accounts.token_program.key(),
            ),
            &[
                ctx.accounts.creator.to_account_info(),
                ctx.accounts.mint.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[seeds_mint],
        )?;

        // initialize mint
        token::initialize_mint2(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                InitializeMint2 {
                    mint: ctx.accounts.mint.to_account_info(),
                },
            ),
            decimals,
            &ctx.accounts.mint_authority.key(),
            Some(&ctx.accounts.freeze_authority.key()),
        )?;

        // mint initial supply to creator’s ATA
        if initial_supply > 0 {
            let seeds_auth: &[&[u8]] = &[
                b"mint_auth",
                &ctx.accounts.mint.key().to_bytes(),
                &[ctx.bumps.mint_authority],
            ];
            token::mint_to(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    MintTo {
                        mint: ctx.accounts.mint.to_account_info(),
                        to: ctx.accounts.creator_ata.to_account_info(),
                        authority: ctx.accounts.mint_authority.to_account_info(),
                    },
                    &[seeds_auth],
                ),
                initial_supply,
            )?;
        }

        // optionally burn mint authority (cannot mint anymore)
        if lock_mint {
            let seeds_auth: &[&[u8]] = &[
                b"mint_auth",
                &ctx.accounts.mint.key().to_bytes(),
                &[ctx.bumps.mint_authority],
            ];
            token::set_authority(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    SetAuthority {
                        account_or_mint: ctx.accounts.mint.to_account_info(),
                        current_authority: ctx.accounts.mint_authority.to_account_info(),
                    },
                    &[seeds_auth],
                ),
                AuthorityType::MintTokens,
                None,
            )?;
        }

        emit!(ForgeCreated {
            mint: forge.mint,
            creator: forge.creator,
            ts: forge.created_at,
        });

        Ok(())
    }
}
