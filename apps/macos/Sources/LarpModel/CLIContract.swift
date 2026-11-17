import Foundation
import CoreFoundation

// Validate the frozen schema before decoding a typed payload. No raw message is presented.
enum CLIContract {
    static let limit = 1_048_576
    static func decimal(_ value: String, nonzero: Bool = false) -> Bool {
        guard !value.isEmpty, value.utf8.allSatisfy({ (48...57).contains($0) }),
              value == "0" || value.first != "0", let n = UInt64(value) else { return false }
        return !nonzero || n > 0
    }
    static func name(_ value: String) -> Bool {
        let reserved = ["larp", "con", "prn", "aux", "nul"] + (1...9).flatMap { ["com\($0)", "lpt\($0)"] }
        return value.range(of: "^[a-z][a-z0-9_-]{0,31}$", options: .regularExpression) != nil && !reserved.contains(value)
    }
    private static let schema: [String: Any] = {
        let url = Bundle.module.url(forResource: "cli-v1.schema", withExtension: "json")!
        return try! JSONSerialization.jsonObject(with: Data(contentsOf: url)) as! [String: Any]
    }()
    private static func valid(_ value: Any, _ rule: [String: Any]) -> Bool {
        if let ref = rule["$ref"] as? String {
            guard let def = (schema["$defs"] as? [String: Any])?[String(ref.dropFirst(8))] as? [String: Any] else { return false }
            if ref == "#/$defs/id", let s = value as? String, !decimal(s, nonzero: true) { return false }
            if ref == "#/$defs/name", let s = value as? String, !name(s) { return false }
            return valid(value, def)
        }
        if let choices = rule["oneOf"] as? [[String: Any]] { return choices.filter { valid(value, $0) }.count == 1 }
        if let choices = rule["anyOf"] as? [[String: Any]] { return choices.contains { valid(value, $0) } }
        func equal(_ a: Any, _ b: Any) -> Bool {
            // NSNumber otherwise considers true equal to 1.
            if let x = a as? NSNumber, let y = b as? NSNumber,
               (CFGetTypeID(x) == CFBooleanGetTypeID()) != (CFGetTypeID(y) == CFBooleanGetTypeID()) { return false }
            return (a as? NSObject)?.isEqual(b) == true
        }
        if let constant = rule["const"], !equal(value, constant) { return false }
        if let choices = rule["enum"] as? [Any], !choices.contains(where: { equal(value, $0) }) { return false }
        switch rule["type"] as? String {
        case "object":
            guard let object = value as? [String: Any], let props = rule["properties"] as? [String: [String: Any]],
                  let required = rule["required"] as? [String], required.allSatisfy({ object[$0] != nil }),
                  Set(object.keys).isSubset(of: Set(props.keys())) else { return false }
            return object.allSatisfy { valid($0.value, props[$0.key]!) }
        case "array":
            guard let array = value as? [Any], let item = rule["items"] as? [String: Any] else { return false }
            if let max = rule["maxItems"] as? Int, array.count > max { return false }
            return array.allSatisfy { valid($0, item) }
        case "string":
            guard let s = value as? String else { return false }
            if let pattern = rule["pattern"] as? String { return s.range(of: pattern, options: .regularExpression)?.lowerBound == s.startIndex && s.range(of: pattern, options: .regularExpression)?.upperBound == s.endIndex }
        case "boolean": return (value as? NSNumber).map { CFGetTypeID($0) == CFBooleanGetTypeID() } ?? false
        case "null": return value is NSNull
        default: break
        }
        return true
    }
    static func decode(_ bytes: Data, exit: Int32) throws -> CLIEnvelope {
        guard bytes.count <= limit, bytes.last == 10, bytes.first == 123, bytes.dropLast().last == 125, String(data: bytes, encoding: .utf8) != nil,
              let object = try? JSONSerialization.jsonObject(with: bytes) as? [String: Any],
              let major = object["schema"] as? NSNumber, CFGetTypeID(major) != CFBooleanGetTypeID(), major == 1,
              valid(object, schema) else { throw ClientError.invalidResponse }
        let envelope = try JSONDecoder().decode(CLIEnvelope.self, from: bytes)
        guard (envelope.ok && exit == 0) || (!envelope.ok && (exit == 1 || exit == 2)) else { throw ClientError.invalidResponse }
        if envelope.ok {
            if envelope.command == "help" || envelope.command == "version" {
                guard envelope.revision == nil else { throw ClientError.invalidResponse }
            } else {
                guard let revision = envelope.revision, decimal(revision) else { throw ClientError.invalidResponse }
            }
            try envelope.validate()
        }
        return envelope
    }
}

struct CLIEnvelope: Decodable {
    let schema: Int
    let command: String
    let ok: Bool
    let revision: String?
    let data: Payload?
    let error: SafeError?
    struct SafeError: Decodable { let code: String; let message: String }
    enum Payload: Decodable {
        case profiles([WireProfile]), profile(WireProfile), aliases([WireAlias]), bindings([Detection])
        case mcp(MCPList), skills(SkillsList), matrix(Matrix), status(Status), mutation(Mutation), receipt(Receipt), text(String), version(String)
        init(from decoder: Decoder) throws { throw ClientError.invalidResponse }
    }
    enum CodingKeys: String, CodingKey { case schema, command, ok, revision, data, error }
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        schema = try c.decode(Int.self, forKey: .schema); command = try c.decode(String.self, forKey: .command)
        ok = try c.decode(Bool.self, forKey: .ok); revision = try c.decodeIfPresent(String.self, forKey: .revision)
        error = try c.decodeIfPresent(SafeError.self, forKey: .error)
        guard ok else { data = nil; return }
        let d = try c.superDecoder(forKey: .data)
        switch command {
        case "profile.list": data = .profiles(try Profiles(from: d).profiles)
        case "profile.show": data = .profile(try WireProfile(from: d))
        case "alias.list": data = .aliases(try Aliases(from: d).aliases)
        case "provider.list": data = .bindings(try Bindings(from: d).bindings)
        case "mcp.list": data = .mcp(try MCPList(from: d))
        case "skills.list": data = .skills(try SkillsList(from: d))
        case "mcp.matrix", "skills.matrix": data = .matrix(try Matrix(from: d))
        case "status", "doctor": data = .status(try Status(from: d))
        case "app.open": data = .receipt(try Receipt(from: d))
        case "help": data = .text(try TextData(from: d).text)
        case "version": data = .version(try VersionData(from: d).version)
        default: data = .mutation(try Mutation(from: d))
        }
    }
    func validate() throws {
        func unique(_ values: [String]) throws { guard Set(values).count == values.count else { throw ClientError.invalidResponse } }
        func profiles(_ p: [WireProfile]) throws {
            try unique(p.map(\.id)); try unique(p.map(\.name))
            try unique(p.flatMap { $0.aliases.map(\.name) })
            for item in p { guard item.aliases.allSatisfy({ $0.profile_id == item.id }) else { throw ClientError.invalidResponse } }
        }
        switch data {
        case let .profiles(p): try profiles(p)
        case let .profile(p): try profiles([p])
        case let .aliases(a): try unique(a.map(\.name))
        case let .bindings(b): try unique(b.map { $0.provider + "." + $0.surface })
        case let .matrix(m):
            try unique(m.profiles.map(\.id)); try unique(m.profiles.map(\.name)); try unique(m.rows.map(\.name))
            guard m.rows.allSatisfy({ $0.enabled.count == m.profiles.count }) else { throw ClientError.invalidResponse }
        case let .mcp(m):
            try unique(m.servers.map(\.name)); for s in m.servers { try unique(s.switches.map(\.profile_id)); try unique(s.secret_refs.map { $0.kind + "." + $0.name }) }
        case let .skills(s): try unique(s.skills.map(\.name)); for item in s.skills { try unique(item.switches.map(\.profile_id)) }
        case let .status(s): if let p = s.profile { try profiles([p]) }; try unique(s.detected_versions.map { $0.provider + "." + $0.surface })
        case let .mutation(m):
            let profileCommands = ["profile.add", "profile.rename", "profile.forget", "profile.delete", "profile.recover", "profile.surface", "alias.add", "alias.remove", "skills.enable", "skills.disable"]
            if profileCommands.contains(command), m.profile_id == nil { throw ClientError.invalidResponse }
            let global = ["profile.migrate", "provider.bind", "app.bind", "mcp.sync", "mcp.sync-all", "skills.sync"]
            if global.contains(command), m.profile_id != nil || m.name != nil { throw ClientError.invalidResponse }
            if !global.contains(command), m.name == nil { throw ClientError.invalidResponse }
        default: break
        }
    }
}
struct Profiles: Decodable { let profiles: [WireProfile] }
struct Aliases: Decodable { let aliases: [WireAlias] }
struct Bindings: Decodable { let bindings: [Detection] }
struct TextData: Decodable { let text: String }
struct VersionData: Decodable { let version: String }
struct WireSurface: Decodable { let enabled: Bool; let path: String; let state: String }
struct WireSurfaces: Decodable { let cli: WireSurface; let desktop: WireSurface }
struct BindingData: Decodable { let path: String?; let recorded_version: String?; let detected_version: String?; let state: String; let qualified: Bool }
struct WireAlias: Decodable { let name: String; let profile_id: String; let surface: String; let path: String; let state: String }
struct WireProfile: Decodable { let id: String; let name: String; let provider: String; let state: String; let surfaces: WireSurfaces; let aliases: [WireAlias]; let desktop_binding: BindingData }
struct Detection: Decodable { let provider: String; let surface: String; let binding: BindingData }
struct WireSwitch: Decodable { let profile_id: String; let enabled: Bool }
struct SecretRef: Decodable { let kind: String; let name: String }
struct Server: Decodable { let name: String; let transport: String; let secret_refs: [SecretRef]; let switches: [WireSwitch]; let state: String }
struct Skill: Decodable { let name: String; let sha256: String; let switches: [WireSwitch]; let state: String }
struct MCPList: Decodable { let sync_all: Bool; let servers: [Server] }
struct SkillsList: Decodable { let skills: [Skill] }
struct Column: Decodable, Equatable { let id: String; let name: String; let provider: String }
struct Matrix: Decodable { let profiles: [Column]; let rows: [Row]; struct Row: Decodable { let name: String; let enabled: [Bool] } }
struct Diagnostic: Decodable { let code: String; let message: String }
struct Status: Decodable { let profile: WireProfile?; let detected_versions: [Detection]; let native_auth: String; let native_rows: [String]; let diagnostics: [Diagnostic] }
struct Mutation: Decodable { let changed: Bool; let profile_id: String?; let name: String?; let warnings: [Diagnostic] }
struct Receipt: Decodable { let profile_id: String; let provider: String; let surface: String; let outcome: String; let native_auth: String }
