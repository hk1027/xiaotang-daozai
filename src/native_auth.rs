//! 只读解密本机 Electron 登录缓存，不续期、不回写原客户端文件。
use base64::{engine::general_purpose::STANDARD,Engine};
use ring::aead;
use std::path::Path;

pub(super) fn decrypt(key:&[u8],encoded:&str)->Result<String,()>{
    let blob=STANDARD.decode(encoded).map_err(|_|())?;
    if !blob.starts_with(b"v10") || blob.len()<31{return Err(());}
    let nonce=aead::Nonce::try_assume_unique_for_key(&blob[3..15]).map_err(|_|())?;
    let key=aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM,key).map_err(|_|())?);
    let mut sealed=blob[15..].to_vec();
    let plain=key.open_in_place(nonce,aead::Aad::empty(),&mut sealed).map_err(|_|())?;
    String::from_utf8(plain.to_vec()).map_err(|_|())
}

#[cfg(windows)]
pub(super) fn key(path:&Path)->Result<Vec<u8>,()>{
    use windows::Win32::{Foundation::{HLOCAL,LocalFree},Security::Cryptography::{CryptUnprotectData,CRYPT_INTEGER_BLOB,CRYPTPROTECT_UI_FORBIDDEN}};
    let raw=std::fs::read(path.with_file_name("Local State")).map_err(|_|())?;
    let value:serde_json::Value=serde_json::from_slice(&raw).map_err(|_|())?;
    let blob=STANDARD.decode(value["os_crypt"]["encrypted_key"].as_str().ok_or(())?).map_err(|_|())?;
    let protected=blob.strip_prefix(b"DPAPI").ok_or(())?;
    let input=CRYPT_INTEGER_BLOB{cbData:protected.len().try_into().map_err(|_|())?,pbData:protected.as_ptr().cast_mut()};
    let mut output=CRYPT_INTEGER_BLOB::default();
    unsafe{CryptUnprotectData(&input,None,None,None,None,CRYPTPROTECT_UI_FORBIDDEN,&mut output).map_err(|_|())?;}
    if output.pbData.is_null(){return Err(());}
    let key=unsafe{std::slice::from_raw_parts(output.pbData,output.cbData as usize).to_vec()};
    unsafe{let _=LocalFree(HLOCAL(output.pbData.cast()));}
    if key.len()!=32{return Err(());}Ok(key)
}
#[cfg(not(windows))]
pub(super) fn key(_path:&Path)->Result<Vec<u8>,()>{Err(())}
