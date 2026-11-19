// Bootstrap only a fresh disposable instance. Keep the generated member credentials outside the repository.
import assert from 'node:assert/strict';
import { randomBytes } from 'node:crypto';
import { writeFile } from 'node:fs/promises';

const [base, output] = process.argv.slice(2);
assert(base && output, 'Usage: node scripts/private-listening/seed.mjs <loopback origin> <fixture.json>');
const origin = new URL(base);
assert(['localhost', '127.0.0.1', '[::1]'].includes(origin.hostname), 'Test instances must be on loopback');

async function api(method, path, body, token) {
  const headers = {};
  if (token) headers.Authorization = 'Bearer ' + token;
  if (body && !(body instanceof FormData)) {
    headers['Content-Type'] = 'application/json';
    body = JSON.stringify(body);
  }
  const response = await fetch(new URL('/api' + path, origin), { method, headers, body, signal: AbortSignal.timeout(45000) });
  if (!response.ok) throw new Error(method + ' ' + path + ': HTTP ' + response.status);
  return response.headers.get('content-type')?.includes('application/json') ? response.json() : null;
}

function recording(frequency, seconds = 24) {
  const rate = 16000;
  const samples = Math.round(rate * seconds);
  const bytes = Buffer.alloc(44 + samples * 2);
  bytes.write('RIFF'); bytes.writeUInt32LE(bytes.length - 8, 4); bytes.write('WAVEfmt ', 8);
  bytes.writeUInt32LE(16, 16); bytes.writeUInt16LE(1, 20); bytes.writeUInt16LE(1, 22);
  bytes.writeUInt32LE(rate, 24); bytes.writeUInt32LE(rate * 2, 28);
  bytes.writeUInt16LE(2, 32); bytes.writeUInt16LE(16, 34); bytes.write('data', 36); bytes.writeUInt32LE(samples * 2, 40);
  for (let n = 0; n < samples; n++) bytes.writeInt16LE(Math.round(1400 * Math.sin(2 * Math.PI * frequency * n / rate)), 44 + 2 * n);
  return new Blob([bytes], { type: 'audio/wav' });
}

async function account(username, path) {
  const password = randomBytes(20).toString('hex');
  const session = await api('POST', path, { username, email: username + '@example.test', password });
  return { username, password, token: session.tokens.access_token };
}

const state = await api('GET', '/setup/status');
assert(!state.has_admin && !state.setup_complete, 'Seeding requires a fresh instance without users');
const admin = await account('fixture-admin', '/setup/admin');
await api('POST', '/setup/instance', { instance_name: 'Private listening test', instance_description: 'Disposable acceptance fixture' }, admin.token);
await api('POST', '/setup/complete', { p2p_enabled: false, open_registrations: true, max_upload_size_mb: 10 }, admin.token);
assert.equal((await api('GET', '/nodeinfo')).open_registration, true);
await api('PUT', '/admin/settings/open_registrations', { value: 'false' }, admin.token);
assert.equal((await api('GET', '/nodeinfo')).open_registration, false);
const registration = () => account('registration-probe', '/auth/register');
await assert.rejects(registration, /HTTP 403/);
await api('PUT', '/admin/settings/open_registrations', { value: 'true' }, admin.token);
const member = await account('listener-one', '/auth/register');
const other = await account('listener-two', '/auth/register');
const album = 'Night Sessions';
const artist = 'SoundTime Ensemble';
async function uploadRecording(title, hz) {
  const data = new FormData();
  data.set('file', recording(hz), title + '.wav');
  data.set('title', title); data.set('album', album); data.set('artist', artist);
  const result = await api('POST', '/upload', data, member.token);
  assert(result.id && result.duration > 10, 'Fixture requires real seekable audio');
  return { id: result.id, title };
}
const tracks = [await uploadRecording('First Light', 196), await uploadRecording('Second Wind', 246)];
const albumId = (await api('GET', '/tracks/' + tracks[0].id, undefined, member.token)).album_id;
assert(albumId, 'Uploaded track must belong to an album');
const playlists = [];
for (const [name, is_public] of [['Listening Notes', false], ['Open Rotation', true]]) {
  const playlist = await api('POST', '/playlists', { name, is_public }, member.token);
  assert(playlist.id);
  await api('POST', '/playlists/' + playlist.id + '/tracks', { track_id: tracks[0].id }, member.token);
  playlists.push({ id: playlist.id, name, is_public });
}
// Preserve a real withdrawn entry between two playable tracks in the owner's private playlist.
const withdrawn = await uploadRecording('Recalled Recording', 329);
const queuePlaylist = playlists.find(playlist => !playlist.is_public);
for (const track of [withdrawn, tracks[1]]) {
  await api('POST', '/playlists/' + queuePlaylist.id + '/tracks', { track_id: track.id }, member.token);
}
await api('DELETE', '/tracks/' + withdrawn.id, undefined, member.token);
await api('PUT', '/admin/settings/instance_private', { value: 'true' }, admin.token);
assert((await api('GET', '/setup/status')).instance_private);
assert.equal((await api('GET', '/nodeinfo')).open_registration, false);
await assert.rejects(registration, /HTTP 403/);
// The second member exercises deep pagination independently of the album used for listening.
for (let start = 0; start < 101; start += 50) {
  const batch = new FormData();
  for (let i = start; i < Math.min(start + 50, 101); i++) {
    batch.append('files', recording(300 + i, 0.125), 'Library sample ' + (i + 1) + '.wav');
  }
  const uploaded = await api('POST', '/upload/batch', batch, other.token);
  assert.equal(uploaded.success, Math.min(50, 101 - start), 'Every library sample must be imported');
}
const lastImportPage = await api('GET', '/tracks/my-uploads?page=3&per_page=50', undefined, other.token);
assert.equal(lastImportPage.total, 101);
assert.equal(lastImportPage.data.length, 1);
const { id: importId, title: importTitle } = lastImportPage.data[0];
const importPageThree = { id: importId, title: importTitle };
const credentials = ({ username, password }) => ({ username, password });
await writeFile(output, JSON.stringify({ origin: origin.origin, importPageThree, admin: credentials(admin), member: credentials(member), other: credentials(other), albumId, album, artist, tracks, playlists, withdrawn }), { flag: 'wx', mode: 0o600 });
console.log('Private instance seeded with two members, a listening album, two playlists and 101 personal imports.');
