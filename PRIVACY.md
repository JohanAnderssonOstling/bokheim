# Bokheim Privacy Notice

Effective: 1 August 2026

Bokheim is an EPUB library and reading application. It can be used with local
books only, or connected to the Bokheim synchronization service.

## Data kept on the device

Bokheim stores application preferences, library metadata, reading positions,
download state, authentication tokens, thumbnails, and imported book files on
the user's device. Application preferences are device-local and are not
synchronized.

## Data sent to the Bokheim service

When a user signs in, their username and password are sent over an encrypted
HTTPS connection for authentication. The password is not stored by the mobile
application; the resulting authentication session is stored locally.

A local library is not uploaded merely because the user signs in. If the user
explicitly adds a library to the server, Bokheim can upload its library and
folder metadata, book metadata, reading state, EPUB files, and generated
thumbnails. The service also processes the network address and normal HTTP
request information needed to provide synchronization and downloads.

Bokheim contains no advertising SDK and does not sell personal data.

## Control and deletion

Users can keep libraries local, choose which libraries are added to the
server, remove downloaded books from a device, and sign out. Signing out
removes the local authenticated session but does not delete data already
stored on the server.

Requests concerning a server account or deletion of synchronized data can be
sent to johan.andersson.ostling@protonmail.com.

## Changes

Material changes to this notice will be published with a new effective date in
the Bokheim source repository and release documentation.
