use pinocchio::{
    entrypoint, error::ProgramError, sysvars::Sysvar, AccountView, Address, ProgramResult,
};
use pinocchio_system::instructions::CreateAccount;

pub const ID: Address = Address::new_from_array(five8_const::decode_32_const(
    "pcWKVSdcdDUKabPz4pVfaQ2jMod1kWv3LqeQivjKXiF",
));

// --- State ---

#[repr(C)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy)]
pub struct PunchcardHeader {
    pub authority: [u8; 32],
    pub capacity: u64,
    pub claimed: u64,
}

pub struct Bits<'a>(&'a mut [u8]);

impl Bits<'_> {
    pub fn get(&self, index: u64) -> bool {
        let byte = self.0[(index / 8) as usize];
        (byte & (1 << (index % 8))) != 0
    }

    pub fn set(&mut self, index: u64) {
        self.0[(index / 8) as usize] |= 1 << (index % 8);
    }
}

pub struct Punchcard<'a> {
    pub header: &'a mut PunchcardHeader,
    pub bits: Bits<'a>,
}

impl<'a> Punchcard<'a> {
    pub fn space(capacity: u64) -> usize {
        size_of::<PunchcardHeader>() + ((capacity as usize + 7) / 8)
    }

    pub fn from_bytes(data: &'a mut [u8]) -> Self {
        let (header, bits) = data.split_at_mut(size_of::<PunchcardHeader>());
        Self {
            header: bytemuck::from_bytes_mut(header),
            bits: Bits(bits),
        }
    }

    pub fn claim(&mut self, index: u64) -> ProgramResult {
        if self.bits.get(index) {
            return Err(Error::AlreadyClaimed.into());
        }
        self.bits.set(index);
        self.header.claimed += 1;
        Ok(())
    }
}

// --- Instructions ---

#[derive(borsh::BorshSerialize, borsh::BorshDeserialize)]
pub enum Instruction {
    Create { capacity: u64 },
    Claim { indices: Vec<u64> },
}

// --- Errors ---

#[repr(u32)]
pub enum Error {
    InvalidAuthority = 0,
    IndexOutOfBounds = 1,
    AlreadyClaimed = 2,
}

impl From<Error> for ProgramError {
    fn from(e: Error) -> Self {
        ProgramError::Custom(e as u32)
    }
}

// --- Processor ---

#[cfg(not(feature = "no-entrypoint"))]
entrypoint!(process);

pub fn process(program_id: &Address, accounts: &[AccountView], data: &[u8]) -> ProgramResult {
    match borsh::from_slice(data).map_err(|_| ProgramError::InvalidInstructionData)? {
        Instruction::Create { capacity } => create(program_id, accounts, capacity),
        Instruction::Claim { indices } => claim(program_id, accounts, &indices),
    }
}

fn create(program_id: &Address, accounts: &[AccountView], capacity: u64) -> ProgramResult {
    let [payer, punchcard, _system] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let space = Punchcard::space(capacity);
    let rent = pinocchio::sysvars::rent::Rent::get()?.try_minimum_balance(space)?;

    CreateAccount {
        from: payer,
        to: punchcard,
        lamports: rent,
        space: space as u64,
        owner: program_id,
    }
    .invoke()?;

    let mut data = punchcard.try_borrow_mut()?;
    let card = Punchcard::from_bytes(&mut data);
    card.header.authority = payer.address().to_bytes();
    card.header.capacity = capacity;
    card.header.claimed = 0;

    Ok(())
}

fn claim(program_id: &Address, accounts: &[AccountView], indices: &[u64]) -> ProgramResult {
    let [authority, punchcard] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !punchcard.owned_by(program_id) {
        return Err(ProgramError::IncorrectProgramId);
    }

    let (capacity, claimed) = {
        let mut data = punchcard.try_borrow_mut()?;
        let mut card = Punchcard::from_bytes(&mut data);

        if card.header.authority != *authority.address().as_ref() {
            return Err(Error::InvalidAuthority.into());
        }

        for &i in indices {
            if i >= card.header.capacity {
                return Err(Error::IndexOutOfBounds.into());
            }
            card.claim(i)?;
        }

        (card.header.capacity, card.header.claimed)
    };

    if claimed == capacity {
        authority.set_lamports(authority.lamports() + punchcard.lamports());
        punchcard.set_lamports(0);
        punchcard.try_borrow_mut()?.fill(0);
        punchcard.close()?;
    }

    Ok(())
}
