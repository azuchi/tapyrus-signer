use crate::cli::setup::traits::Response;
use crate::crypto::vss::{Commitment, Vss};
use crate::errors::Error;
use bitcoin::{PrivateKey, PublicKey};
use clap::{App, Arg, ArgMatches, SubCommand};
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

pub struct CreateNodeVssResponse {
    vss: BTreeMap<PublicKey, Vss>,
}

impl CreateNodeVssResponse {
    fn new(vss: BTreeMap<PublicKey, Vss>) -> Self {
        CreateNodeVssResponse { vss: vss }
    }
}

impl Response for CreateNodeVssResponse {}

impl fmt::Display for CreateNodeVssResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = self
            .vss
            .iter()
            .map(|(k, v)| format!("{}:{}", k, v))
            .collect::<Vec<String>>()
            .join("\n");
        write!(f, "{}", c)
    }
}

pub struct CreateNodeVssCommand {}

impl<'a> CreateNodeVssCommand {
    pub fn execute(matches: &ArgMatches) -> Result<Box<dyn Response>, Error> {
        let private_key: PrivateKey = matches
            .value_of("private_key")
            .and_then(|key| PrivateKey::from_wif(key).ok())
            .ok_or(Error::InvalidArgs("private_key".to_string()))?;

        let mut public_keys: Vec<PublicKey> = matches
            .values_of("public_key")
            .ok_or(Error::InvalidArgs("public_key".to_string()))?
            .map(|key| PublicKey::from_str(key).map_err(|_| Error::InvalidKey))
            .collect::<Result<Vec<PublicKey>, _>>()?;
        public_keys.sort();

        let (vss_scheme, secret_shares) = Vss::create_node_shares(&private_key, public_keys.len());

        let mut vss_map = BTreeMap::new();
        let secp = secp256k1::Secp256k1::new();
        let sender_public_key = PublicKey::from_private_key(&secp, &private_key);
        let commitments: Vec<Commitment> = vss_scheme
            .commitments
            .iter()
            .map(|c| Commitment::from(c))
            .collect();
        for j in 0..public_keys.len() {
            let vss = Vss::new(
                sender_public_key,
                public_keys[j].clone(),
                commitments.clone(),
                secret_shares[j],
                commitments.clone(),
                secret_shares[j],
            );
            vss_map.insert(public_keys[j].clone(), vss);
        }
        Ok(Box::new(CreateNodeVssResponse::new(vss_map)))
    }

    pub fn args<'b>() -> App<'a, 'b> {
        SubCommand::with_name("createnodevss").args(&[
            Arg::with_name("public_key")
                .long("public_key")
                .required(true)
                .multiple(true)
                .takes_value(true)
                .help("compressed public key of the each signer with a hex format string"),
            Arg::with_name("private_key")
                .long("private_key")
                .required(true)
                .takes_value(true)
                .help("private key of this signer with an extend WIF format"),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute() {
        let matches = CreateNodeVssCommand::args().get_matches_from(vec![
            "createnodevss",
            "--public_key", "03842d51608d08bee79587fb3b54ea68f5279e13fac7d72515a7205e6672858ca2",
            "--public_key", "03e568e3a5641ac21930b51f92fb6dd201fb46faae560b108cf3a96380da08dee1",
            "--public_key", "02a1c8965ed06987fa6d7e0f552db707065352283ab3c1471510b12a76a5905287",
            "--private_key", "cQYYBMFS9dRR3Mt16gW4jixCqSiMhCwuDMHUBs6WeHMTxMnsq8Gh"
        ]);
        let response = CreateNodeVssCommand::execute(&matches);
        assert!(response.is_ok());
    }

    #[test]
    fn test_execute_invalid_public_key() {
        let matches = CreateNodeVssCommand::args().get_matches_from(vec![
            "createnodevss",
            "--public_key", "x",
            "--private_key", "cQYYBMFS9dRR3Mt16gW4jixCqSiMhCwuDMHUBs6WeHMTxMnsq8Gh"
        ]);
        let response = CreateNodeVssCommand::execute(&matches);
        assert_eq!(format!("{}", response.err().unwrap()), "InvalidKey");
    }

    #[test]
    fn test_execute_invalid_private_key() {
        let matches = CreateNodeVssCommand::args().get_matches_from(vec![
            "createnodevss",
            "--public_key", "03842d51608d08bee79587fb3b54ea68f5279e13fac7d72515a7205e6672858ca2",
            "--private_key", "x"
        ]);
        let response = CreateNodeVssCommand::execute(&matches);
        assert_eq!(format!("{}", response.err().unwrap()), "InvalidArgs(\"private_key\")");
    }
}
