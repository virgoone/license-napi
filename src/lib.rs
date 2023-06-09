#![deny(clippy::all)]

#[macro_use]
extern crate napi_derive;

use aes_gcm::{
  aead::{generic_array::GenericArray, Aead, AeadCore, KeyInit, OsRng},
  Aes256Gcm,
  Key, // Or `Aes128Gcm`
  Nonce,
};
use base64;
use mac_address::get_mac_address;
use napi::{CallContext, Either, Env, JsBuffer, JsObject, JsString, Result};
use rsa::{
  pkcs1::DecodeRsaPrivateKey, pkcs8::DecodePublicKey, Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey,
};
use serde_derive::{Deserialize, Serialize};
use std::string::String;
use std::{fs, str};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
#[napi(object)]
pub struct DeviceObject {
  pub mac_address: String,

  pub serial_number: String,
}

#[napi(object)]
pub struct MachineObject {
  pub key: String,

  pub nonce: String,

  pub device_id: String,
}

#[napi]
pub fn sum(a: i32, b: i32) -> i32 {
  a + b
}
#[cfg(target_os = "linux")]
fn has_dmidecode() -> bool {
  use std::process::Command;
  match Command::new("dmidecode").arg("--version").output() {
    Ok(_) => true,
    Err(_) => false,
  }
}

#[cfg(target_os = "linux")]
// #[cfg(target_os = "macos")]
fn get_serial_number() -> String {
  use std::{
    fs::{create_dir_all, File},
    io::{Read, Write},
    path::Path,
  };
  let home_config = Path::new("~/.config");

  if !home_config.exists() {
    if let Err(e) = create_dir_all(home_config) {
      println!("Error creating ~/.config: {}", e);
    }
  }

  let path = Path::new("~/.config/license_serial_number");

  if path.exists() {
    // Read UUID from file
    let mut file = File::open(path).expect("Failed to open UUID file");
    let mut contents = String::new();
    file
      .read_to_string(&mut contents)
      .expect("Failed to read UUID file");
    contents.trim().to_string()
  } else {
    // Generate UUID and save to file
    let uuid = Uuid::new_v4().to_string();
    match File::create(path) {
      Ok(mut file) => {
        file.write(uuid.as_bytes()).expect("Failed to write UUID");
      }
      Err(e) => {
        println!("create file Error{}", e)
      }
    };
    // let mut file = File::create(path).expect("Failed to create UUID file");
    // file.write(uuid.as_bytes()).expect("Failed to write UUID");
    uuid
  }
}

#[cfg(target_os = "macos")]
fn get_serial_number() -> String {
  use std::process::Command;

  let output = Command::new("ioreg")
    .arg("-l")
    .output()
    .expect("Failed to execute command");

  let output_str = String::from_utf8_lossy(&output.stdout);
  output_str
    .lines()
    .find(|line| line.contains("IOPlatformSerialNumber"))
    .map(|line| line.split('"').nth(3).unwrap().trim())
    .unwrap_or("Unknown")
    .into()
}

#[cfg(target_os = "windows")]
fn get_serial_number() -> String {
  use std::process::Command;

  let output = Command::new("wmic")
    .arg("baseboard")
    .arg("get")
    .arg("SerialNumber")
    .output()
    .expect("Failed to execute command");

  let output_str = String::from_utf8_lossy(&output.stdout);
  output_str
    .lines()
    .nth(1)
    .map(|line| line.trim().to_string())
    .unwrap_or("Unknown".into())
}

pub fn get_sys_info_result() -> (Option<String>, Option<String>) {
  let serial_number = get_serial_number();
  let mac_address = get_mac_address().ok().map(|mac| mac.unwrap().to_string());

  (Some(serial_number), mac_address)
}

#[napi]
pub fn uuid_v4(env: Env) -> Result<JsString> {
  let uuid = Uuid::new_v4();
  Ok(env.create_string(&uuid.to_string())?.into())
}

#[napi]
pub fn get_sys_info(env: Env) -> Result<JsObject> {
  let (serial_number, mac_address) = get_sys_info_result();

  let mut result = env.create_object()?;
  result.set_named_property(
    "serial-number",
    env.create_string(&serial_number.unwrap_or("".to_string()))?,
  )?;
  result.set_named_property(
    "mac-address",
    env.create_string(&mac_address.unwrap_or("".to_string()))?,
  )?;

  Ok(result)
}

fn encrypt_string(public_key: &RsaPublicKey, plain: &[u8]) -> String {
  let mut rng = rand::thread_rng();
  let enc_data: Vec<u8> = public_key
    .encrypt(&mut rng, Pkcs1v15Encrypt, &plain[..])
    .expect("failed to encrypt");
  let encoded = base64::encode(enc_data);
  encoded
}

fn decrypt_string(private_key: &RsaPrivateKey, enc_data: &[u8]) -> String {
  // 使用私钥解密
  let decrypted = private_key
    .decrypt(Pkcs1v15Encrypt, &enc_data)
    .expect("failed to decrypt");
  println!("Successfully decrypted data");
  // 将解密后的结果转为String
  String::from_utf8(decrypted).unwrap()
}

fn decrypt_bytes(private_key: &RsaPrivateKey, enc_data: &[u8]) -> Vec<u8> {
  // 使用私钥解密
  let decrypted = private_key
    .decrypt(Pkcs1v15Encrypt, &enc_data)
    .expect("failed to decrypted");
  println!("Successfully decrypted data");
  // 将解密后的结果转为String
  decrypted
}

#[napi]
pub fn encrypt(env: Env, pem: String, key: String) -> Result<JsString> {
  let public_key = RsaPublicKey::from_public_key_pem(&pem).expect("Failed to parse PEM");
  let encoded_str: String = encrypt_string(&public_key, &key.as_bytes());
  Ok(env.create_string(&encoded_str)?.into())
}

#[napi]
pub fn decrypt(env: Env, pem: String, key: String) -> Result<JsString> {
  let private_key = RsaPrivateKey::from_pkcs1_pem(&pem).expect("Failed to parse PEM");
  let _key: Vec<u8> = base64::decode(&key).unwrap();
  let decoded_str: String = decrypt_string(&private_key, &_key);
  env.create_string(&decoded_str.as_ref())
}

#[napi]
pub fn get_machine_id(env: Env, pem: String) -> Result<JsObject> {
  let public_key = RsaPublicKey::from_public_key_pem(&pem).expect("Failed to parse PEM");
  let (serial_number, mac_address) = get_sys_info_result();

  let encoded_serial_number = serial_number.unwrap_or("".to_string());
  let encoded_mac_address = mac_address.unwrap_or("".to_string());
  // The encryption key can be generated randomly:
  let key = Aes256Gcm::generate_key(OsRng);
  let cipher = Aes256Gcm::new(&key);
  let nonce = Aes256Gcm::generate_nonce(&mut OsRng); // 96-bits; unique per message

  let device = DeviceObject {
    mac_address: encoded_mac_address,
    serial_number: encoded_serial_number,
  };
  let encoded;
  if let Ok(json) = serde_json::to_string(&device) {
    let encoded_str = encrypt_string(&public_key, &json.as_bytes());
    let ciphertext = cipher
      .encrypt(&nonce, encoded_str.as_ref())
      .expect("Failed to encrypt");
    encoded = base64::encode(ciphertext);
    // encoded = encrypt_string(&public_key, &json.as_bytes());
  } else {
    encoded = "".to_string();
  }

  let mut result = env.create_object()?;
  result.set_named_property("device_id", env.create_string(&encoded)?)?;
  result.set_named_property("nonce", encrypt_string(&public_key, &nonce[..]))?;
  result.set_named_property("key", encrypt_string(&public_key, &key[..]))?;
  Ok(result)
}

#[napi]
pub fn decrypt_device_id(
  env: Env,
  pem: String,
  device_id: String,
  key: String,
  nonce: String,
) -> Result<JsObject> {
  let private_key = RsaPrivateKey::from_pkcs1_pem(&pem).expect("Failed to parse PEM");
  let _nonce = base64::decode(nonce).unwrap();
  let _key = base64::decode(key).unwrap();
  let _aes_device_id = base64::decode(&device_id).unwrap();
  let decrypted_key = decrypt_bytes(&private_key, &_key);
  let decrypted_nonce = decrypt_bytes(&private_key, &_nonce);

  let _bytes_key = Key::<Aes256Gcm>::from_slice(&decrypted_key[..]);
  let _bytes_nonce = GenericArray::from_slice(&decrypted_nonce);
  let cipher = Aes256Gcm::new(&_bytes_key);
  let cipher_text = cipher
    .decrypt(&_bytes_nonce, _aes_device_id.as_ref())
    .expect("Failed to decrypt aes");

  let _device_id = base64::decode(cipher_text).unwrap();
  let decoded = decrypt_string(&private_key, &_device_id);
  let json: DeviceObject = serde_json::from_str(&decoded).unwrap();

  let mut result = env.create_object()?;
  result.set_named_property("mac_address", env.create_string(&json.mac_address)?)?;
  result.set_named_property("serial_number", env.create_string(&json.serial_number)?)?;

  Ok(result)
}
