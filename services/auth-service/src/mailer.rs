use anyhow::{Context, Result, anyhow};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    time::{Duration, timeout},
};

use crate::config::Config;

const SMTP_TIMEOUT: Duration = Duration::from_secs(5);

pub async fn send_signup_otp(config: &Config, recipient: &str, otp: &str) -> Result<()> {
    if recipient.contains(['\r', '\n']) {
        return Err(anyhow!("invalid recipient"));
    }
    let address = config
        .smtp_from
        .rsplit_once('<')
        .and_then(|(_, value)| value.strip_suffix('>'))
        .unwrap_or(&config.smtp_from);
    if address.contains(['\r', '\n']) {
        return Err(anyhow!("invalid sender"));
    }

    let stream = timeout(
        SMTP_TIMEOUT,
        TcpStream::connect((&*config.smtp_host, config.smtp_port)),
    )
    .await
    .context("SMTP connection timed out")??;
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    expect_code(&mut reader, 220).await?;
    command(&mut reader, &mut writer, "EHLO localhost\r\n", 250).await?;
    command(
        &mut reader,
        &mut writer,
        &format!("MAIL FROM:<{address}>\r\n"),
        250,
    )
    .await?;
    command(
        &mut reader,
        &mut writer,
        &format!("RCPT TO:<{recipient}>\r\n"),
        250,
    )
    .await?;
    command(&mut reader, &mut writer, "DATA\r\n", 354).await?;

    let body = format!(
        "From: {}\r\nTo: {}\r\nSubject: Your Drone Drop verification code\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nYour Drone Drop verification code is: {otp}\r\n\r\nIt expires in {} minutes. If you did not request this, you can ignore this email.\r\n.\r\n",
        config.smtp_from,
        recipient,
        config.otp_ttl_secs / 60
    );
    writer.write_all(body.as_bytes()).await?;
    writer.flush().await?;
    expect_code(&mut reader, 250).await?;
    writer.write_all(b"QUIT\r\n").await?;
    writer.flush().await?;
    Ok(())
}

async fn command<R: AsyncBufReadExt + Unpin, W: AsyncWriteExt + Unpin>(
    reader: &mut R,
    writer: &mut W,
    command: &str,
    expected: u16,
) -> Result<()> {
    writer.write_all(command.as_bytes()).await?;
    writer.flush().await?;
    expect_code(reader, expected).await
}

async fn expect_code<R: AsyncBufReadExt + Unpin>(reader: &mut R, expected: u16) -> Result<()> {
    let mut line = String::new();
    loop {
        line.clear();
        timeout(SMTP_TIMEOUT, reader.read_line(&mut line))
            .await
            .context("SMTP response timed out")??;
        if line.len() < 3 {
            return Err(anyhow!("invalid SMTP response"));
        }
        let code = line[..3]
            .parse::<u16>()
            .context("invalid SMTP response code")?;
        if !line.as_bytes().get(3).is_some_and(|byte| *byte == b'-') {
            if code != expected {
                return Err(anyhow!("SMTP returned {code}, expected {expected}"));
            }
            return Ok(());
        }
    }
}
