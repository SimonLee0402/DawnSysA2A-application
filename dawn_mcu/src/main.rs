use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

mod signer;

// Shared AP2 Structures matching DawnCore
#[derive(Debug, Serialize, Deserialize)]
pub struct PaymentRequest {
    pub transaction_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub mandate_id: Uuid,
    pub amount: f64,
    pub description: String,
    pub mcu_public_did: Option<String>,
    // Provide the hardware signature when approving
    pub mcu_signature: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PaymentResponse {
    pub status: String,
    pub transaction_id: Uuid,
    pub verification_message: String,
}

fn main() {
    println!("=== DawnMCU (ESP32 Simulation) Started ===");
    println!("Hardware physical signing module initialized.");

    // Generate a physical private key for this "MCU"
    let signing_key = signer::generate_mcu_keypair();
    let public_did = signer::get_public_did(&signing_key);
    println!("MCU Public DID Key: {public_did}");

    let client = Client::new();
    let api_url = "http://127.0.0.1:8000/api/ap2/authorize";

    // Simulation loop: MCU periodically checks if there are pending transactions needing physical approval
    // (In a real scenario, this could be triggered via MQTT or WebSocket push)
    loop {
        // Simulating a random intercepted payload from the DawnCore
        let pending_request = PaymentRequest {
            transaction_id: None,
            task_id: None,
            mandate_id: Uuid::new_v4(),
            amount: 50.0,
            description: "A2A Agent requested Server Cloud Deployment".to_string(),
            mcu_public_did: None,
            mcu_signature: None,
        };

        println!("Creating pending AP2 transaction in DawnCore...");
        let create_response = match client.post(api_url).json(&pending_request).send() {
            Ok(resp) => match resp.json::<PaymentResponse>() {
                Ok(parsed) => parsed,
                Err(error) => {
                    println!("Failed to parse DawnCore response: {error}");
                    thread::sleep(Duration::from_secs(10));
                    continue;
                }
            },
            Err(error) => {
                println!("Failed to reach DawnCore: {error}");
                thread::sleep(Duration::from_secs(10));
                continue;
            }
        };

        println!("\n[!] MCU SCREEN ALERT [!]");
        println!("AP2 Transaction Pending Physical Auth:");
        println!("Transaction: {}", create_response.transaction_id);
        println!("Desc: {}", pending_request.description);
        println!("Amount: ${:.2}", pending_request.amount);

        print!("Press 'Y' (Simulate Physical Button) to authorize with VDC Signature, or any other key to reject: ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        if input.trim().eq_ignore_ascii_case("y") {
            println!("Physical Button Pressed! Signing VDC...");

            let payload_to_sign = format!(
                "{}:{}:{:.4}:{}",
                create_response.transaction_id,
                pending_request.mandate_id,
                pending_request.amount,
                pending_request.description
            );
            let signature = signer::sign_payload(&signing_key, &payload_to_sign);

            let approved_request = PaymentRequest {
                transaction_id: Some(create_response.transaction_id),
                task_id: pending_request.task_id,
                mcu_public_did: Some(public_did.clone()),
                mcu_signature: Some(signature.clone()),
                ..pending_request
            };

            println!("Sending signed Auth to DawnCore...");
            match client.post(api_url).json(&approved_request).send() {
                Ok(resp) => match resp.json::<PaymentResponse>() {
                    Ok(parsed) => {
                        println!("DawnCore status: {}", parsed.status);
                        println!("Verification: {}", parsed.verification_message);
                    }
                    Err(error) => println!("Failed to parse DawnCore approval response: {error}"),
                },
                Err(error) => println!("Failed to reach DawnCore for signed auth: {error}"),
            }
            println!("Transaction signed with: {}", signature);
        } else {
            println!("Transaction Rejected by Physical User.");
        }

        println!("Sleeping for 10 seconds before next check...");
        thread::sleep(Duration::from_secs(10));
    }
}
