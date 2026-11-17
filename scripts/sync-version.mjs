import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const packagePath = path.join(root, 'package.json')
const tauriConfigPath = path.join(root, 'src-tauri', 'tauri.conf.json')
const cargoTomlPath = path.join(root, 'src-tauri', 'Cargo.toml')
const cargoLockPath = path.join(root, 'src-tauri', 'Cargo.lock')

const packageJson = JSON.parse(fs.readFileSync(packagePath, 'utf8'))
const version = packageJson.version
if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error(`Invalid package version: ${version}`)
}

function writeIfChanged(filePath, content) {
  if (fs.readFileSync(filePath, 'utf8') !== content) {
    fs.writeFileSync(filePath, content)
  }
}

const tauriConfig = JSON.parse(fs.readFileSync(tauriConfigPath, 'utf8'))
tauriConfig.version = version
writeIfChanged(tauriConfigPath, `${JSON.stringify(tauriConfig, null, 2)}\n`)

const cargoVersionRe = /^(version\s*=\s*")[^"]+("\s*)$/m
const cargoToml = fs.readFileSync(cargoTomlPath, 'utf8')
if (!cargoVersionRe.test(cargoToml)) {
  throw new Error('Could not find the package version in src-tauri/Cargo.toml')
}
const nextCargoToml = cargoToml.replace(
  cargoVersionRe,
  (_, prefix, suffix) => `${prefix}${version}${suffix}`,
)
writeIfChanged(cargoTomlPath, nextCargoToml)

const lockVersionRe = /(\[\[package\]\]\r?\nname = "snapgit"\r?\nversion = ")[^"]+("\s*)/
const cargoLock = fs.readFileSync(cargoLockPath, 'utf8')
if (!lockVersionRe.test(cargoLock)) {
  throw new Error('Could not find the snapgit package version in src-tauri/Cargo.lock')
}
const nextCargoLock = cargoLock.replace(
  lockVersionRe,
  (_, prefix, suffix) => `${prefix}${version}${suffix}`,
)
writeIfChanged(cargoLockPath, nextCargoLock)

console.log(`SnapGit version synchronized to ${version}`)
