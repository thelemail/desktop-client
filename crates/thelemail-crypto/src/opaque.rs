use argon2::Argon2;
use generic_array::{ArrayLength, GenericArray};
use opaque_ke::errors::InternalError;
use opaque_ke::ksf::Ksf;
use opaque_ke::{
    CipherSuite, ClientLogin, ClientLoginFinishParameters, ClientLoginFinishResult,
    ClientLoginStartResult, ClientRegistration, ClientRegistrationFinishParameters,
    CredentialResponse, Identifiers, RegistrationResponse,
};
use rand::rngs::OsRng;

pub const ARGON2_M_COST: u32 = 65536;
pub const ARGON2_T_COST: u32 = 3;
pub const ARGON2_P_COST: u32 = 4;

pub struct MemoryConstrainedKsf {
    argon: Argon2<'static>,
}

impl Default for MemoryConstrainedKsf {
    fn default() -> Self {
        let params = argon2::Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, None)
            .expect("argon2 parameters");
        Self {
            argon: Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params),
        }
    }
}

impl Ksf for MemoryConstrainedKsf {
    fn hash<L: ArrayLength<u8>>(
        &self,
        input: GenericArray<u8, L>,
    ) -> Result<GenericArray<u8, L>, InternalError> {
        let mut output = GenericArray::<u8, L>::default();
        self.argon
            .hash_password_into(&input, &[0u8; argon2::RECOMMENDED_SALT_LEN], &mut output)
            .map_err(|_| InternalError::KsfError)?;
        Ok(output)
    }
}

pub type LoginState = ClientLogin<ThelemailSuite>;
pub type RegistrationState = ClientRegistration<ThelemailSuite>;

pub struct ThelemailSuite;

impl CipherSuite for ThelemailSuite {
    type OprfCs = opaque_ke::Ristretto255;
    type KeyExchange = opaque_ke::TripleDh<opaque_ke::Ristretto255, sha2::Sha512>;
    type Ksf = MemoryConstrainedKsf;
}

#[derive(Debug, thiserror::Error)]
pub enum OpaqueError {
    #[error("invalid protocol message")]
    InvalidMessage,
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("protocol failure")]
    Protocol,
}

pub struct LoginStart {
    pub state: ClientLogin<ThelemailSuite>,
    pub ke1: Vec<u8>,
}

pub fn start_login(password: &str) -> Result<LoginStart, OpaqueError> {
    let mut rng = OsRng;
    let ClientLoginStartResult { state, message } =
        ClientLogin::<ThelemailSuite>::start(&mut rng, password.as_bytes())
            .map_err(|_| OpaqueError::Protocol)?;
    Ok(LoginStart {
        state,
        ke1: message.serialize().to_vec(),
    })
}

pub struct LoginFinish {
    pub ke3: Vec<u8>,
    pub export_key: Vec<u8>,
    pub session_key: Vec<u8>,
}

pub fn finish_login(
    state: ClientLogin<ThelemailSuite>,
    password: &str,
    ke2: &[u8],
    client_identity: &str,
    server_identity: &str,
) -> Result<LoginFinish, OpaqueError> {
    let response = CredentialResponse::deserialize(ke2).map_err(|_| OpaqueError::InvalidMessage)?;
    let ClientLoginFinishResult {
        message,
        export_key,
        session_key,
        ..
    } = state
        .finish(
            &mut OsRng,
            password.as_bytes(),
            response,
            ClientLoginFinishParameters::new(
                None,
                Identifiers {
                    client: Some(client_identity.as_bytes()),
                    server: Some(server_identity.as_bytes()),
                },
                Some(&MemoryConstrainedKsf::default()),
            ),
        )
        .map_err(|_| OpaqueError::InvalidCredentials)?;

    Ok(LoginFinish {
        ke3: message.serialize().to_vec(),
        export_key: export_key.to_vec(),
        session_key: session_key.to_vec(),
    })
}

pub struct RegistrationStart {
    pub state: RegistrationState,
    pub request: Vec<u8>,
}

pub fn start_registration(password: &str) -> Result<RegistrationStart, OpaqueError> {
    let mut rng = OsRng;
    let result = ClientRegistration::<ThelemailSuite>::start(&mut rng, password.as_bytes())
        .map_err(|_| OpaqueError::Protocol)?;
    Ok(RegistrationStart {
        state: result.state,
        request: result.message.serialize().to_vec(),
    })
}

pub struct RegistrationFinish {
    pub record: Vec<u8>,
    pub export_key: Vec<u8>,
}

pub fn finish_registration(
    state: ClientRegistration<ThelemailSuite>,
    password: &str,
    response: &[u8],
    client_identity: &str,
    server_identity: &str,
) -> Result<RegistrationFinish, OpaqueError> {
    let parsed =
        RegistrationResponse::deserialize(response).map_err(|_| OpaqueError::InvalidMessage)?;
    let result = state
        .finish(
            &mut OsRng,
            password.as_bytes(),
            parsed,
            ClientRegistrationFinishParameters::new(
                Identifiers {
                    client: Some(client_identity.as_bytes()),
                    server: Some(server_identity.as_bytes()),
                },
                Some(&MemoryConstrainedKsf::default()),
            ),
        )
        .map_err(|_| OpaqueError::Protocol)?;
    Ok(RegistrationFinish {
        record: result.message.serialize().to_vec(),
        export_key: result.export_key.to_vec(),
    })
}
