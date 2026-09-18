package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"time"

	"github.com/ProtonMail/go-crypto/openpgp"
	"github.com/ProtonMail/go-crypto/openpgp/armor"
	"github.com/ProtonMail/go-crypto/openpgp/packet"
)

func readArmoredEntity(armored string) (*openpgp.Entity, error) {
	block, err := armor.Decode(bytes.NewReader([]byte(armored)))
	if err != nil {
		return nil, err
	}
	list, err := openpgp.ReadKeyRing(block.Body)
	if err != nil {
		return nil, err
	}
	if len(list) == 0 {
		return nil, fmt.Errorf("no entity")
	}
	return list[0], nil
}

func encryptToRecipient(armoredPubKey string, plaintext []byte, aead bool) ([]byte, error) {
	entity, err := readArmoredEntity(armoredPubKey)
	if err != nil {
		return nil, err
	}
	now := time.Now
	var out bytes.Buffer
	cfg := &packet.Config{DefaultCipher: packet.CipherAES256, Time: now}
	if aead {
		cfg.AEADConfig = &packet.AEADConfig{}
	}
	enc, err := openpgp.Encrypt(&out, []*openpgp.Entity{entity}, nil, nil, cfg)
	if err != nil {
		return nil, err
	}
	if _, err := io.Copy(enc, bytes.NewReader(plaintext)); err != nil {
		return nil, err
	}
	if err := enc.Close(); err != nil {
		return nil, err
	}
	return out.Bytes(), nil
}

func main() {
	root := os.Args[1]
	variant := "v4"
	if len(os.Args) > 2 {
		variant = os.Args[2]
	}
	suffix, producer, aead := "", "ProtonMail/go-crypto openpgp.Encrypt, packet.Config{DefaultCipher: AES256}, no AEADConfig", false
	switch variant {
	case "v4":
	case "v6":
		suffix, producer, aead = "-v6", "ProtonMail/go-crypto openpgp.Encrypt, packet.Config{DefaultCipher: AES256, AEADConfig: &AEADConfig{}}", true
	default:
		panic("unknown variant " + variant)
	}
	pub, err := os.ReadFile(filepath.Join(root, "keys", "account"+suffix+".pub.asc"))
	if err != nil {
		panic(err)
	}

	large := make([]byte, 512*1024)
	for i := range large {
		large[i] = byte('a' + i%26)
	}

	cases := map[string]string{
		"body-plain-go": "Subject: fixture\r\n\r\nplain body produced by the server encryptor\r\n",
		"preview-go":    `{"v":1,"subject":"Server preview","snippet":"produced by pgpencrypt"}`,
		"body-large-go": string(large),
	}

	meta := map[string]any{}
	for name, plaintext := range cases {
		ct, err := encryptToRecipient(string(pub), []byte(plaintext), aead)
		if err != nil {
			panic(err)
		}
		if err := os.WriteFile(filepath.Join(root, "messages", name+suffix+".pgp"), ct, 0o644); err != nil {
			panic(err)
		}
		meta[name] = map[string]any{
			"plaintextLen":  len(plaintext),
			"producer":      producer,
			"pkeskTag":      int(ct[0] >> 2 & 0x0f),
			"pkeskVersion":  int(ct[2]),
			"ciphertextLen": len(ct),
		}
		fmt.Printf("%-16s ct=%7d first=0x%02x pkeskVersion=%d\n", name, len(ct), ct[0], ct[2])
	}
	out, _ := json.MarshalIndent(meta, "", "\t")
	_ = os.WriteFile(filepath.Join(root, "messages", "go-meta"+suffix+".json"), append(out, '\n'), 0o644)
}
