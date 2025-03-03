use anchor_lang::prelude::*;

declare_id!("2G2gUi3YjhJoziTg4xqeKoB2Y7ReJLUsw52gqYn9FeSP");

#[program]
// State modules
pub mod state {
    pub mod fpl {
        use anchor_lang::prelude::*;

        #[account]
        pub struct FplUser {
            pub authority: Pubkey,       // User's wallet address
            pub fpl_id: String,          // User's FPL ID
            pub team_data: Vec<u8>,      // Serialized team data
            pub weekly_score: u32,       // Current weekly score
            pub total_score: u32,        // Total season score
            pub last_updated: i64,       // Timestamp of last update
            pub bump: u8,                // PDA bump
        }

        #[account]
        pub struct FplGlobalState {
            pub admin: Pubkey,           // Admin authority
            pub current_gameweek: u8,    // Current FPL gameweek
            pub season_start: i64,       // Season start timestamp
            pub season_end: i64,         // Season end timestamp
            pub api_url: String,         // External FPL API URL
            pub bump: u8,                // PDA bump
        }
    }

    pub mod stake {
        use anchor_lang::prelude::*;

        #[account]
        pub struct Stake {
            pub owner: Pubkey,           // Stake owner
            pub amount: u64,             // Staked amount in SOL (lamports)
            pub start_time: i64,         // Start timestamp
            pub lock_period: u64,        // Lock period in seconds
            pub fpl_user: Pubkey,        // Associated FPL user account
            pub is_active: bool,         // Whether stake is active
            pub last_claim_time: i64,    // Last reward claim timestamp
            pub bump: u8,                // PDA bump
        }

        #[account]
        pub struct StakeConfig {
            pub admin: Pubkey,           // Admin authority
            pub min_stake_amount: u64,   // Minimum stake amount
            pub max_stake_amount: u64,   // Maximum stake amount
            pub early_withdrawal_fee: u8, // % fee for early withdrawal (0-100)
            pub lock_options: Vec<u64>,  // Available lock periods in seconds
            pub bump: u8,                // PDA bump
        }

        #[account]
        pub struct StakeCount {
            pub count: u64,
        }
    }

    pub mod treasury {
        use anchor_lang::prelude::*;

        #[account]
        pub struct Treasury {
            pub admin: Pubkey,           // Admin authority
            pub total_fees: u64,         // Total collected fees
            pub protocol_fee: u8,        // Protocol fee percentage (0-100)
            pub reserve_percentage: u8,  // Percentage to keep as reserves
            pub bump: u8,                // PDA bump
        }
    }
}

// Instructions module
pub mod instructions {
    pub mod fpl {
        use anchor_lang::prelude::*;
        use crate::state::fpl::*;

        pub struct FplGlobalParams {
            pub current_gameweek: u8,
            pub season_start: i64,
            pub season_end: i64, 
            pub api_url: String,
        }

        #[derive(Accounts)]
        pub struct InitializeFplGlobal<'info> {
            #[account(mut)]
            pub admin: Signer<'info>,
            
            #[account(
                init,
                payer = admin,
                space = 8 + 32 + 1 + 8 + 8 + 100 + 1,
                seeds = [b"fpl-global"],
                bump
            )]
            pub global_state: Account<'info, FplGlobalState>,
            
            pub system_program: Program<'info, System>,
        }

        pub fn initialize_fpl_global(ctx: Context<InitializeFplGlobal>, params: FplGlobalParams) -> Result<()> {
            let global_state = &mut ctx.accounts.global_state;
            let bump = ctx.bumps.global_state;            
            global_state.admin = ctx.accounts.admin.key();
            global_state.current_gameweek = params.current_gameweek;
            global_state.season_start = params.season_start;
            global_state.season_end = params.season_end;
            global_state.api_url = params.api_url;
            global_state.bump = bump;
            
            Ok(())
        }

        #[derive(Accounts)]
        pub struct RegisterFplUser<'info> {
            #[account(mut)]
            pub user: Signer<'info>,
            
            #[account(
                init,
                payer = user,
                space = 8 + 32 + 50 + 200 + 4 + 4 + 8 + 1,
                seeds = [b"fpl-user", user.key().as_ref()],
                bump
            )]
            pub fpl_user: Account<'info, FplUser>,
            
            pub global_state: Account<'info, FplGlobalState>,
            pub system_program: Program<'info, System>,
        }

        pub fn register_fpl_user(ctx: Context<RegisterFplUser>, fpl_id: String) -> Result<()> {
            let fpl_user = &mut ctx.accounts.fpl_user;
            let bump = ctx.bumps.fpl_user;
            
            if fpl_id.len() == 0 || fpl_id.len() > 20 {
                return Err(crate::errors::ErrorCode::InvalidFplId.into());
            }
            
            fpl_user.authority = ctx.accounts.user.key();
            fpl_user.fpl_id = fpl_id;
            fpl_user.team_data = Vec::new();
            fpl_user.weekly_score = 0;
            fpl_user.total_score = 0;
            fpl_user.last_updated = Clock::get()?.unix_timestamp;
            fpl_user.bump = bump;
            
            Ok(())
        }
    }

    pub mod stake {      
        use anchor_lang::prelude::*;
        use crate::state::fpl::FplUser;
        use crate::state::stake::{Stake, StakeConfig, StakeCount};
        use crate::state::treasury::Treasury;
        use crate::errors::ErrorCode;

        #[derive(Accounts)]
        pub struct CreateStake<'info> {
            #[account(mut)]
            pub user: Signer<'info>,
            
            #[account(
                mut,
                seeds = [b"stake", user.key().as_ref(), &stake_count.count.to_le_bytes()],
                bump
            )]
            pub stake: Account<'info, Stake>,
            
            pub stake_config: Account<'info, StakeConfig>,
            
            #[account(mut)]
            pub stake_count: Account<'info, StakeCount>,
            
            pub fpl_user: Account<'info, FplUser>,
            
            /// CHECK: This is the PDA that holds the staked SOL
            #[account(mut)]
            pub stake_vault: UncheckedAccount<'info>,
            
            pub system_program: Program<'info, System>,
        }

        pub fn create_stake(ctx: Context<CreateStake>, amount: u64, lock_period: u64) -> Result<()> {
            // Get current timestamp
            let current_time = Clock::get()?.unix_timestamp;
            
            // Validate stake amount
            if amount < ctx.accounts.stake_config.min_stake_amount || 
               amount > ctx.accounts.stake_config.max_stake_amount {
                return Err(ErrorCode::InvalidStakeAmount.into());
            }
            
            // Validate lock period is one of the allowed options
            if !ctx.accounts.stake_config.lock_options.contains(&lock_period) {
                return Err(ErrorCode::InvalidLockPeriod.into());
            }
            
            // Transfer SOL from user to stake vault
            let transfer_instruction = anchor_lang::solana_program::system_instruction::transfer(
                ctx.accounts.user.key,
                ctx.accounts.stake_vault.key,
                amount,
            );
            
            anchor_lang::solana_program::program::invoke(
                &transfer_instruction,
                &[
                    ctx.accounts.user.to_account_info(),
                    ctx.accounts.stake_vault.to_account_info(),
                    ctx.accounts.system_program.to_account_info(),
                ],
            )?;
            
            // Initialize stake account
            let stake = &mut ctx.accounts.stake;
            stake.owner = ctx.accounts.user.key();
            stake.amount = amount;
            stake.start_time = current_time;
            stake.lock_period = lock_period;
            stake.fpl_user = ctx.accounts.fpl_user.key();
            stake.is_active = true;
            stake.last_claim_time = current_time;
            stake.bump = ctx.bumps.stake;
            
            // Increment stake count
            ctx.accounts.stake_count.count += 1;
            
            Ok(())
        }


    #[derive(Accounts)]
    #[instruction(stake_id: u64)]
    pub struct Unstake<'info> {
        #[account(mut)]
        pub user: Signer<'info>,
        
        #[account(
            mut,
            seeds = [b"stake", user.key().as_ref(), &stake_id.to_le_bytes()],
            bump = stake.bump,
            constraint = stake.owner == user.key() @ ErrorCode::UnauthorizedAccess,
            constraint = stake.is_active @ ErrorCode::StakeNotActive
        )]
        pub stake: Account<'info, Stake>,
        
        pub stake_config: Account<'info, StakeConfig>,
        
        /// CHECK: This is the PDA that holds the staked SOL
        #[account(mut)]
        pub stake_vault: UncheckedAccount<'info>,
        
        #[account(mut)]
        pub treasury: Account<'info, Treasury>,
        
        pub system_program: Program<'info, System>,
    }

    pub fn unstake(ctx: Context<Unstake>, _stake_id: u64) -> Result<()> {
        // Get current timestamp
        let current_time = Clock::get()?.unix_timestamp;
        
        // Calculate stake end time
        let stake_end_time = ctx.accounts.stake.start_time + ctx.accounts.stake.lock_period as i64;
        
        // Check if lock period has ended and calculate fees if not
        let mut fee_amount: u64 = 0;
        let mut return_amount = ctx.accounts.stake.amount;
        
        if current_time < stake_end_time {
            // Early withdrawal - calculate fee
            fee_amount = (ctx.accounts.stake.amount as u128 * ctx.accounts.stake_config.early_withdrawal_fee as u128 / 100) as u64;
            return_amount = return_amount.saturating_sub(fee_amount);
        }
        
        // Get stake vault bump to build the PDA for signing
        let (stake_vault_pda, stake_vault_bump) = 
            Pubkey::find_program_address(&[b"stake-vault"], ctx.program_id);
        
        if ctx.accounts.stake_vault.key() != stake_vault_pda {
            return Err(ErrorCode::UnauthorizedAccess.into());
        }
        
        // Create longer-lived signing data
        let stake_vault_bytes = b"stake-vault";
        let stake_vault_bump_bytes = [stake_vault_bump];
        let signing_seeds = vec![&stake_vault_bytes[..], &stake_vault_bump_bytes[..]];
        let signer = &[&signing_seeds[..]];
        
        // Transfer SOL back to user
        if return_amount > 0 {
            anchor_lang::solana_program::program::invoke_signed(
                &anchor_lang::solana_program::system_instruction::transfer(
                    ctx.accounts.stake_vault.key,
                    ctx.accounts.user.key,
                    return_amount,
                ),
                &[
                    ctx.accounts.stake_vault.to_account_info(),
                    ctx.accounts.user.to_account_info(),
                    ctx.accounts.system_program.to_account_info(),
                ],
                signer,
            )?;
        }
        
        // If there's a fee, transfer it to treasury
        if fee_amount > 0 {
            // Update treasury stats
            ctx.accounts.treasury.total_fees = ctx.accounts.treasury.total_fees.saturating_add(fee_amount);
            
            // Transfer fees to treasury vault
            let treasury_pda = Pubkey::find_program_address(&[b"treasury-vault"], ctx.program_id).0;
            
            anchor_lang::solana_program::program::invoke_signed(
                &anchor_lang::solana_program::system_instruction::transfer(
                    ctx.accounts.stake_vault.key,
                    &treasury_pda,
                    fee_amount,
                ),
                &[
                    ctx.accounts.stake_vault.to_account_info(),
                    ctx.accounts.treasury.to_account_info(),
                    ctx.accounts.system_program.to_account_info(),
                ],
                signer,
            )?;
        }
        
        // Mark stake as inactive
        ctx.accounts.stake.is_active = false;
        
        Ok(())
    }
      
    }
}

// Errors module
pub mod errors {
    use anchor_lang::prelude::*;

    #[error_code]
    pub enum ErrorCode {
        #[msg("Invalid FPL ID")]
        InvalidFplId,
        #[msg("Invalid stake amount")]
        InvalidStakeAmount,
        #[msg("Insufficient funds")]
        InsufficientFunds,
        #[msg("Invalid lock period")]
        InvalidLockPeriod,
        #[msg("Unauthorized access")]
        UnauthorizedAccess,
        #[msg("Stake not active")]
        StakeNotActive,
    }
}

// Program implementation module
pub mod program_impl {
    use super::*;
    use crate::instructions::fpl::*;
    use crate::instructions::stake::*;

    pub fn initialize_fpl_global(ctx: Context<InitializeFplGlobal>, params: FplGlobalParams) -> Result<()> {
        instructions::fpl::initialize_fpl_global(ctx, params)
    }

    pub fn register_fpl_user(ctx: Context<RegisterFplUser>, fpl_id: String) -> Result<()> {
        instructions::fpl::register_fpl_user(ctx, fpl_id)
    }

    pub fn create_stake(ctx: Context<CreateStake>, amount: u64, lock_period: u64) -> Result<()> {
        instructions::stake::create_stake(ctx, amount, lock_period)
    }

    pub fn unstake(ctx: Context<Unstake>, stake_id: u64) -> Result<()> {
        instructions::stake::unstake(ctx, stake_id)
    }
}pub struct Initialize {}