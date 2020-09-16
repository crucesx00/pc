# Protocol Notes

## First Contact

When the server is running and the first client connects, it performs the following handshake:

```
Client -> Server: identity
Server -> Client: Ack
```

where the `identity` is the fingerprint of the public key that the user is using as their identity.

The server, at that point, labels the connection with the identity of the user.

## Second User Joins

When the second user joins, a slightly different thing happens:

```
1. Client 2 -> Server: identity(client_2)
2. Server -> Client 1: new_user(identity(client_2))
3. Client 1 -> Client 2: encrypted(client_2_pub, identity(client_1))
4. Client 2 -> Client 1: encrypted(client_1_pub, challenge(nonce))
5. Client 1 -> Client 2: encrypted(client_2_pub, response(nonce, session_key))
6. Client 1 -> Server: add_user(client_2_identity)
7. Server -> broadcast: user_added(client_2_identity)
8. Client 2 -> broadcast: encrypted(session_key, hello_i_am(name, identity))
```

1. Second user identifies itself to the server
2. Server tells first user about the new user
    - First user looks up second user in their trusted identities and finds their public key
    - If they don't find the user, they send a Nack to the server. The server then tries another user, until
     one user can identify the new user. If nobody can, the server disconnects the new user
3. First user sends their identity, encrypted with the new user's public key
    - New user looks up user's identity and gets their public key. If they can't find it, they are warned and the client disconnects
4. New user sends a nonce to the first user, encrypted with the first user's public key
5. The first user returns the (decrypted) nonce and the session key, both encrypted with the second user's public key
6. The first user tells the server to add the second user to the chat session
7. The server adds the client, and sends a message to the client that they have been added
8. The new user broadcasts a greeting with their identity, encrypted with the session key

## Messages

### identity(user_identity)
User sends identity

### new_user(identity)
Identify a new user wanting to join the chat

### challenge(nonce)
Challenge with a nonce

### response(nonce, session_key)
Challenge response

### add_user(identity)
Tell the server to add a user

### user_added(identity)
User has been added

### hello_i_am(name, identity)
Introduce yourself
