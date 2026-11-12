package main

import (
	"bufio"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"os"

	"github.com/bytemare/ecc"
	lib "github.com/bytemare/opaque"
)

const (
	clientPrefix   = "thelemail/auth/opaque/v1:"
	recoveryInfix  = "recovery:"
	serverIdentity = "thelemail.com"
)

type request struct {
	Op        string `json:"op"`
	AccountID string `json:"accountId"`
	Message   string `json:"message"`
	Record    string `json:"record"`
	Recovery  bool   `json:"recovery"`
}

type reply struct {
	Message string `json:"message,omitempty"`
	Error   string `json:"error,omitempty"`
}

func newServer() (*lib.Server, error) {
	conf := lib.DefaultConfiguration()
	conf.Context = nil
	group := conf.AKE.Group()
	sk := group.NewScalar().Random()
	seed := make([]byte, 64)
	if _, err := rand.Read(seed); err != nil {
		return nil, err
	}
	srv, err := conf.Server()
	if err != nil {
		return nil, err
	}
	err = srv.SetKeyMaterial(&lib.ServerKeyMaterial{
		Identity:       []byte(serverIdentity),
		PrivateKey:     sk,
		PublicKeyBytes: group.Base().Multiply(sk).Encode(),
		OPRFGlobalSeed: seed,
	})
	return srv, err
}

func credentialID(r request) []byte {
	if r.Recovery {
		return []byte(recoveryInfix + r.AccountID)
	}
	return []byte(r.AccountID)
}

func clientIdentity(r request) []byte {
	if r.Recovery {
		return []byte(clientPrefix + recoveryInfix + r.AccountID)
	}
	return []byte(clientPrefix + r.AccountID)
}

func handle(srv *lib.Server, r request) (string, error) {
	raw, err := base64.StdEncoding.DecodeString(r.Message)
	if err != nil {
		return "", err
	}
	switch r.Op {
	case "register":
		parsed, err := srv.Deserialize.RegistrationRequest(raw)
		if err != nil {
			return "", err
		}
		resp, err := srv.RegistrationResponse(parsed, credentialID(r), (*ecc.Scalar)(nil))
		if err != nil {
			return "", err
		}
		return base64.StdEncoding.EncodeToString(resp.Serialize()), nil
	case "login":
		ke1, err := srv.Deserialize.KE1(raw)
		if err != nil {
			return "", err
		}
		rawRecord, err := base64.StdEncoding.DecodeString(r.Record)
		if err != nil {
			return "", err
		}
		record, err := srv.Deserialize.RegistrationRecord(rawRecord)
		if err != nil {
			return "", err
		}
		msg, _, err := srv.GenerateKE2(ke1, &lib.ClientRecord{
			RegistrationRecord:   record,
			CredentialIdentifier: credentialID(r),
			ClientIdentity:       clientIdentity(r),
		})
		if err != nil {
			return "", err
		}
		return base64.StdEncoding.EncodeToString(msg.Serialize()), nil
	}
	return "", fmt.Errorf("unknown op %q", r.Op)
}

func main() {
	srv, err := newServer()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	in := bufio.NewScanner(os.Stdin)
	in.Buffer(make([]byte, 0, 1<<16), 1<<22)
	out := json.NewEncoder(os.Stdout)
	for in.Scan() {
		var r request
		var rep reply
		if err := json.Unmarshal(in.Bytes(), &r); err != nil {
			rep.Error = err.Error()
		} else if msg, err := handle(srv, r); err != nil {
			rep.Error = err.Error()
		} else {
			rep.Message = msg
		}
		if err := out.Encode(rep); err != nil {
			os.Exit(1)
		}
	}
}
