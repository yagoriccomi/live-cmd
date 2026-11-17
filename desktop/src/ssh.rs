//! Destino SSH da tela "Servidores SSH": valida usuário, host, porta e chave e monta a linha de
//! comando do `ssh.exe` do Windows argumento a argumento (sem shell no meio). Nunca há senha aqui:
//! só o caminho da chave privada, se houver.
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Ssh {
    pub user: String,
    pub host: String,
    pub port: u16,
    /// Caminho absoluto da chave privada (`-i`); `None` = chaves padrão do ssh/agente.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

/// Nome DNS, IPv4 ou IPv6 sem colchetes. Não começa com `-` (viraria opção do ssh).
pub fn valid_host(h: &str) -> bool {
    (1..=253).contains(&h.len()) && !h.starts_with('-')
        && h.bytes().all(|c| c.is_ascii_alphanumeric() || b".-_:".contains(&c))
}

/// Usuário POSIX/Windows simples: letras, dígitos, `.`, `_`, `-`; não começa com `-`.
pub fn valid_user(u: &str) -> bool {
    (1..=64).contains(&u.len()) && !u.starts_with('-')
        && u.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}

/// Caminho absoluto do Windows (`C:\...` ou `\\servidor\...`), sem aspas nem caracteres de controle.
pub fn valid_key(k: &str) -> bool {
    let b = k.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/');
    let unc = k.starts_with(r"\\") && k.len() > 2;
    (drive || unc) && k.chars().count() <= 1024 && !k.chars().any(|c| c.is_control() || c == '"')
}

impl Ssh {
    /// Código do primeiro campo inválido.
    pub fn check(&self) -> Result<(), &'static str> {
        if !valid_user(&self.user) { return Err("INVALID_SSH_USER"); }
        if !valid_host(&self.host) { return Err("INVALID_SSH_HOST"); }
        if self.port == 0 { return Err("INVALID_SSH_PORT"); }
        if self.key.as_deref().is_some_and(|k| !valid_key(k)) { return Err("INVALID_SSH_KEY"); }
        Ok(())
    }

    /// Argumentos do `ssh.exe`, um por item; o host vai por último, depois das opções.
    pub fn args(&self) -> Vec<String> {
        let mut a = vec!["-p".to_owned(), self.port.to_string(), "-l".to_owned(), self.user.clone()];
        if let Some(k) = &self.key { a.extend(["-i".to_owned(), k.clone()]); }
        a.push(self.host.clone());
        a
    }

    /// Linha para `CreateProcessW`: o ssh.exe direto, sem `cmd.exe`.
    pub fn command_line(&self) -> String {
        std::iter::once(exe()).chain(self.args()).map(|a| quote(&a)).collect::<Vec<_>>().join(" ")
    }
}

/// `ssh.exe` do OpenSSH do Windows; sem ele, o primeiro do PATH.
pub fn exe() -> String {
    std::env::var_os("SystemRoot").map(|r| std::path::PathBuf::from(r).join(r"System32\OpenSSH\ssh.exe"))
        .filter(|p| p.is_file()).map_or_else(|| "ssh.exe".to_owned(), |p| p.to_string_lossy().into_owned())
}

/// Um argumento nas regras do `CommandLineToArgvW`/CRT: aspas só se preciso; barras antes de aspas dobram.
pub fn quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\x0b', '"']) { return arg.to_owned(); }
    let mut s = String::from('"');
    let mut slashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => { slashes += 1; continue; }
            '"' => s.push_str(&"\\".repeat(slashes * 2 + 1)),
            _ => s.push_str(&"\\".repeat(slashes)),
        }
        slashes = 0;
        s.push(c);
    }
    s.push_str(&"\\".repeat(slashes * 2));
    s.push('"');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ssh(user: &str, host: &str, port: u16, key: Option<&str>) -> Ssh {
        Ssh { user: user.into(), host: host.into(), port, key: key.map(Into::into) }
    }

    #[test]
    fn valida_campos() {
        for h in ["srv.exemplo.com", "10.0.0.5", "fe80::1", "meu_host", "a"] { assert!(valid_host(h), "{h}"); }
        for h in ["", "-oProxyCommand=x", "a b", "a;b", "a&b", "a|b", "$(x)", "a`b", "h\n", "u@h", "\"h\"", &"a".repeat(254)] {
            assert!(!valid_host(h), "{h:?}");
        }
        for u in ["root", "yago.r", "deploy-01", "ci_user"] { assert!(valid_user(u), "{u}"); }
        for u in ["", "-l", "a b", "a;b", "u@h", "dom\\u", "a'b", &"u".repeat(65)] { assert!(!valid_user(u), "{u:?}"); }
        for k in [r"C:\Users\y\.ssh\id_ed25519", r"C:\Minhas Chaves\id", "D:/k/id", r"\\nas\chaves\id"] { assert!(valid_key(k), "{k}"); }
        for k in ["", "id_rsa", r".\id", "-oProxyCommand=x", "C:\\a\"b", "C:\\a\nb", r"\\"] { assert!(!valid_key(k), "{k:?}"); }
        assert_eq!(ssh("u", "h", 22, None).check(), Ok(()));
        assert_eq!(ssh("u", "h", 0, None).check(), Err("INVALID_SSH_PORT"));
        assert_eq!(ssh("u;x", "h", 22, None).check(), Err("INVALID_SSH_USER"));
        assert_eq!(ssh("u", "h x", 22, None).check(), Err("INVALID_SSH_HOST"));
        assert_eq!(ssh("u", "h", 22, Some("id")).check(), Err("INVALID_SSH_KEY"));
    }

    #[test]
    fn argumentos_em_lista() {
        assert_eq!(ssh("root", "10.0.0.5", 2222, None).args(), ["-p", "2222", "-l", "root", "10.0.0.5"]);
        assert_eq!(ssh("u", "h", 22, Some(r"C:\Minhas Chaves\id")).args(), ["-p", "22", "-l", "u", "-i", r"C:\Minhas Chaves\id", "h"]);
        let line = ssh("u", "h", 22, Some(r"C:\Minhas Chaves\id")).command_line();
        assert!(line.ends_with(r#" -p 22 -l u -i "C:\Minhas Chaves\id" h"#), "{line}");
        assert!(line.to_ascii_lowercase().contains("ssh.exe") && !line.contains("cmd"), "{line}");
    }

    #[test]
    fn aspas_do_windows() {
        assert_eq!(quote("abc"), "abc");
        assert_eq!(quote(r"C:\a\b"), r"C:\a\b");
        assert_eq!(quote(""), r#""""#);
        assert_eq!(quote("a b"), r#""a b""#);
        assert_eq!(quote(r"C:\a b\"), r#""C:\a b\\""#);
        assert_eq!(quote(r#"a"b"#), r#""a\"b""#);
        assert_eq!(quote(r#"a\"b"#), r#""a\\\"b""#);
    }

    #[test]
    fn json_sem_campos_extras() {
        let s: Ssh = serde_json::from_str(r#"{"user":"u","host":"h","port":22}"#).unwrap();
        assert_eq!(s, ssh("u", "h", 22, None));
        assert!(serde_json::from_str::<Ssh>(r#"{"user":"u","host":"h","port":22,"password":"x"}"#).is_err());
        assert!(serde_json::from_str::<Ssh>(r#"{"user":"u","host":"h","port":70000}"#).is_err());
    }
}
