use pinocchio::{
    account_info::AccountInfo, entrypoint, program_error::ProgramError, pubkey::Pubkey,
    sysvars::Sysvar, ProgramResult,
};
use pinocchio_system::instructions::CreateAccount;

pub const ID: Pubkey = five8_const::decode_32_const("pcWKVSdcdDUKabPz4pVfaQ2jMod1kWv3LqeQivjKXiF");

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

const PUNCHCARD_HEADER_LEN: usize = size_of::<PunchcardHeader>();

fn bitset_len(capacity: u64) -> Option<usize> {
    let capacity = usize::try_from(capacity).ok()?;
    capacity.checked_add(7).map(|value| value / 8)
}

impl<'a> Punchcard<'a> {
    pub fn space(capacity: u64) -> Option<usize> {
        let bits_len = bitset_len(capacity)?;
        PUNCHCARD_HEADER_LEN.checked_add(bits_len)
    }

    fn split(data: &'a mut [u8]) -> Result<(&'a mut PunchcardHeader, &'a mut [u8]), ProgramError> {
        if data.len() < PUNCHCARD_HEADER_LEN {
            return Err(ProgramError::InvalidAccountData);
        }

        let (header, bits) = data.split_at_mut(PUNCHCARD_HEADER_LEN);
        let header =
            bytemuck::try_from_bytes_mut(header).map_err(|_| ProgramError::InvalidAccountData)?;

        Ok((header, bits))
    }

    pub fn from_bytes(data: &'a mut [u8]) -> Result<Self, ProgramError> {
        let (header, bits) = Self::split(data)?;
        let expected_bits_len =
            bitset_len(header.capacity).ok_or(Error::InvalidCapacity.into_program_error())?;

        if bits.len() != expected_bits_len {
            return Err(ProgramError::InvalidAccountData);
        }
        if header.claimed > header.capacity {
            return Err(ProgramError::InvalidAccountData);
        }

        Ok(Self {
            header,
            bits: Bits(bits),
        })
    }

    pub fn claim(&mut self, index: u64) -> ProgramResult {
        if self.bits.get(index) {
            return Err(Error::AlreadyClaimed.into_program_error());
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
    InvalidCapacity = 3,
}

impl Error {
    pub fn into_program_error(self) -> ProgramError {
        ProgramError::Custom(self as u32)
    }
}

// --- Processor ---

#[cfg(not(feature = "no-entrypoint"))]
entrypoint!(process);

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    match borsh::from_slice(data).map_err(|_| ProgramError::InvalidInstructionData)? {
        Instruction::Create { capacity } => create(program_id, accounts, capacity),
        Instruction::Claim { indices } => claim(program_id, accounts, &indices),
    }
}

fn create(program_id: &Pubkey, accounts: &[AccountInfo], capacity: u64) -> ProgramResult {
    let [payer, punchcard, _system] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let space = Punchcard::space(capacity).ok_or(Error::InvalidCapacity.into_program_error())?;
    let rent = pinocchio::sysvars::rent::Rent::get()?.minimum_balance(space);

    CreateAccount {
        from: payer,
        to: punchcard,
        lamports: rent,
        space: space as u64,
        owner: program_id,
    }
    .invoke()?;

    let mut data = punchcard.try_borrow_mut_data()?;
    let (header, bits) = Punchcard::split(&mut data)?;
    header.authority = *payer.key();
    header.capacity = capacity;
    header.claimed = 0;
    bits.fill(0);

    Ok(())
}

fn claim(program_id: &Pubkey, accounts: &[AccountInfo], indices: &[u64]) -> ProgramResult {
    let [authority, punchcard] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !punchcard.is_owned_by(program_id) {
        return Err(ProgramError::IncorrectProgramId);
    }

    let (capacity, claimed) = {
        let mut data = punchcard.try_borrow_mut_data()?;
        let mut card = Punchcard::from_bytes(&mut data)?;

        if card.header.authority != *authority.key() {
            return Err(Error::InvalidAuthority.into_program_error());
        }

        for &i in indices {
            if i >= card.header.capacity {
                return Err(Error::IndexOutOfBounds.into_program_error());
            }
            card.claim(i)?;
        }

        (card.header.capacity, card.header.claimed)
    };

    if claimed == capacity {
        let punchcard_lamports = punchcard.lamports();
        *authority.try_borrow_mut_lamports()? += punchcard_lamports;
        *punchcard.try_borrow_mut_lamports()? = 0;
        punchcard.try_borrow_mut_data()?.fill(0);
        punchcard.close()?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_rejects_overflowing_capacities() {
        assert_eq!(Punchcard::space(u64::MAX), None);
        assert_eq!(Punchcard::space(u64::MAX - 1), None);
        assert_eq!(Punchcard::space(u64::MAX - 6), None);
        assert!(Punchcard::space(u64::MAX - 7).is_some());
    }

    #[test]
    fn from_bytes_rejects_mismatched_bitset_length() {
        let mut data = vec![0u8; PUNCHCARD_HEADER_LEN + 1];
        let (header, _) = data.split_at_mut(PUNCHCARD_HEADER_LEN);
        let header = bytemuck::from_bytes_mut::<PunchcardHeader>(header);
        header.capacity = 0;
        header.claimed = 0;

        assert!(matches!(
            Punchcard::from_bytes(&mut data),
            Err(ProgramError::InvalidAccountData)
        ));
    }

    #[test]
    fn from_bytes_rejects_claimed_greater_than_capacity() {
        let mut data = vec![0u8; PUNCHCARD_HEADER_LEN];
        let header = bytemuck::from_bytes_mut::<PunchcardHeader>(&mut data[..PUNCHCARD_HEADER_LEN]);
        header.capacity = 0;
        header.claimed = 1;

        assert!(matches!(
            Punchcard::from_bytes(&mut data),
            Err(ProgramError::InvalidAccountData)
        ));
    }
}
