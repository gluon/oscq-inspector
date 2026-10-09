//! Prints the full address space of an OSCQuery server, and optionally sends one value.
//!
//! ```text
//! cargo run -p oscq-core --example oscq_probe -- http://HOST:PORT
//! cargo run -p oscq-core --example oscq_probe -- http://HOST:PORT --send /path ARG...
//! ```
//!
//! Each ARG is parsed against the node's declared `TYPE`, one per top level type tag. `T`, `F`,
//! `N` and `I` take an empty string, `""`.

use anyhow::{Context, Result, bail};
use oscq_core::model::{ArgEntry, Node};
use oscq_core::osc::arg_from_text;
use oscq_core::{HostInfo, OscQueryClient, OscUdpSender, PortSource};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (base, send) = match args.as_slice() {
        [base] => (base.clone(), None),
        [base, flag, path, rest @ ..] if flag == "--send" => {
            (base.clone(), Some((path.clone(), rest.to_vec())))
        }
        _ => bail!("usage: oscq_probe http://HOST:PORT [--send /path ARG...]"),
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run(&base, send))
}

async fn run(base: &str, send: Option<(String, Vec<String>)>) -> Result<()> {
    let client = OscQueryClient::new(url::Url::parse(base).context("server URL")?)?;

    let host_info = match client.fetch_host_info().await {
        Ok((info, diags)) => {
            print_host_info(&info, &client);
            for d in diags {
                println!("  warning: HOST_INFO {:?}", d.kind);
            }
            Some(info)
        }
        Err(e) => {
            println!("HOST_INFO unavailable, browsable but not controllable: {e}");
            None
        }
    };
    let value_supported = host_info.as_ref().is_some_and(|h| h.supports("VALUE"));

    let doc = client.fetch_tree("/").await?;
    println!();
    println!("{} nodes, {} bytes", doc.report.root.count(), doc.raw.len());
    print_tree(&doc.report.root, value_supported);
    for d in &doc.report.diagnostics {
        println!("  warning: {} {:?}", d.path, d.kind);
    }

    let Some((path, texts)) = send else {
        return Ok(());
    };
    let Some(info) = host_info else {
        bail!("cannot send: HOST_INFO unavailable");
    };
    let dest = info.osc_destination(client.http_host(), client.http_port())?;
    let node = doc
        .report
        .root
        .find(&path)
        .with_context(|| format!("no node {path} in the tree"))?;
    if !node.effective_access(value_supported).writable() {
        bail!("{path} is not writable");
    }
    let osc_path = node
        .path
        .as_ref()
        .with_context(|| format!("{path} is not a valid OSC address"))?;
    let sig = node
        .signature
        .clone()
        .unwrap_or(oscq_core::TypeSignature::parse("")?);
    if texts.len() != sig.args().len() {
        bail!(
            "{path} declares TYPE {:?}, expected {} arguments, got {}",
            sig.as_str(),
            sig.args().len(),
            texts.len()
        );
    }
    let osc_args = sig
        .args()
        .iter()
        .zip(&texts)
        .map(|(ty, t)| arg_from_text(ty, t))
        .collect::<Result<Vec<_>, _>>()?;
    let addr = OscUdpSender::new()
        .send_message(&dest, osc_path, &sig, osc_args)
        .await?;
    println!();
    println!("sent {path} {texts:?} to {addr} over UDP");
    Ok(())
}

fn print_host_info(info: &HostInfo, client: &OscQueryClient) {
    println!("server  {}", info.name.as_deref().unwrap_or("(no NAME)"));
    match info.osc_destination(client.http_host(), client.http_port()) {
        Ok(d) => {
            let source = match d.port_source {
                PortSource::Advertised => "advertised",
                PortSource::InferredFromHttp => "inferred from the HTTP port",
            };
            let sendable = match d.check_sendable() {
                Ok(()) => "sendable".to_owned(),
                Err(e) => format!("read only: {e}"),
            };
            println!(
                "osc     {}:{} {:?}, port {source}, {sendable}",
                d.host, d.port, d.transport
            );
        }
        Err(e) => println!("osc     not controllable: {e}"),
    }
    let (ws_host, ws_port, advertised) =
        info.websocket_address(client.http_host(), client.http_port());
    println!(
        "ws      {ws_host}:{ws_port} ({})",
        if advertised {
            "advertised"
        } else {
            "default, not advertised"
        }
    );
    let ext: Vec<&str> = info
        .extensions
        .iter()
        .filter(|(_, on)| **on)
        .map(|(k, _)| k.as_str())
        .collect();
    println!("ext     {}", ext.join(" "));
}

fn print_tree(root: &Node, value_supported: bool) {
    let mut stack = vec![(root, 0usize)];
    while let Some((node, depth)) = stack.pop() {
        let access = node.effective_access(value_supported);
        let flags = format!(
            "{}{}",
            if access.readable() { "r" } else { "-" },
            if access.writable() { "w" } else { "-" }
        );
        let value = node
            .value
            .as_ref()
            .map(|v| format!(" = {}", value_text(v)))
            .unwrap_or_default();
        println!(
            "{:indent$}{} {} {}{}",
            "",
            node.full_path,
            node.type_raw.as_deref().unwrap_or("-"),
            flags,
            value,
            indent = depth * 2
        );
        let children: Vec<_> = node.children().collect();
        for child in children.into_iter().rev() {
            stack.push((child, depth + 1));
        }
    }
}

fn value_text(v: &oscq_core::PerArg<serde_json::Value>) -> String {
    fn entry(e: &ArgEntry<serde_json::Value>) -> String {
        match e {
            ArgEntry::Null => "null".to_owned(),
            ArgEntry::Value(v) | ArgEntry::Invalid(v) => v.to_string(),
            ArgEntry::Nested(items) => {
                format!(
                    "[{}]",
                    items.iter().map(entry).collect::<Vec<_>>().join(", ")
                )
            }
        }
    }
    match v {
        oscq_core::PerArg::List(items) => {
            format!(
                "[{}]",
                items.iter().map(entry).collect::<Vec<_>>().join(", ")
            )
        }
        oscq_core::PerArg::Uniform(e) => entry(e),
    }
}
