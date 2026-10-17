import CryptoKit
import Foundation

// "password" actions store the password AES-256-GCM encrypted in deck.json, as base64 of
// nonce + ciphertext + tag (the standard combined form, readable by any AES-GCM library).
// The key is a separate file next to deck.json, readable only by you. That keeps passwords
// out of deck.json if it's shared or backed up, but anyone who can read both files can decrypt.

let keyURL = Config.url.deletingLastPathComponent().appending(path: "secret.key")

/// Loads the key, or makes one the first time a password is saved.
func passwordKey(create: Bool = false) -> SymmetricKey? {
    if let data = try? Data(contentsOf: keyURL), let raw = Data(base64Encoded: data), raw.count == 32 {
        return SymmetricKey(data: raw)
    }
    guard create else { return nil }
    let key = SymmetricKey(size: .bits256)
    do {
        try FileManager.default.createDirectory(at: keyURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        let raw = key.withUnsafeBytes { Data($0) }.base64EncodedData()
        guard FileManager.default.createFile(atPath: keyURL.path, contents: raw, attributes: [.posixPermissions: 0o600]) else {
            throw CocoaError(.fileWriteUnknown)
        }
    } catch {
        logger.error("Cannot write \(keyURL.path, privacy: .public): \(error, privacy: .public)")
        return nil
    }
    return key
}

func encryptPassword(_ password: String, key: SymmetricKey? = passwordKey(create: true)) -> String? {
    guard let key else { return nil }
    return try? AES.GCM.seal(Data(password.utf8), using: key).combined?.base64EncodedString()
}

func decryptPassword(_ sealed: String, key: SymmetricKey? = passwordKey()) -> String? {
    guard let key, let data = Data(base64Encoded: sealed),
          let box = try? AES.GCM.SealedBox(combined: data),
          let plain = try? AES.GCM.open(box, using: key) else { return nil }
    return String(data: plain, encoding: .utf8)
}
