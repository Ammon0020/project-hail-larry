// Package server provides the HTTP server that serves the web UI and API.
// This file implements self-signed TLS certificate generation for LAN access
// (Blueprint Sec 19 — TLS on LAN).
package server

import (
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/pem"
	"fmt"
	"log"
	"math/big"
	"net"
	"os"
	"path/filepath"
	"time"
)

const (
	// certValidity is the self-signed certificate validity period (1 year).
	certValidity = 365 * 24 * time.Hour
	// certFilePerm restricts the private key file to the owner only.
	certFilePerm = 0600
	// certDirPerm restricts the cert directory to the owner only.
	certDirPerm = 0700
)

// EnsureSelfSignedCert generates a self-signed ECDSA P-256 TLS certificate
// valid for localhost, 127.0.0.1, and all non-loopback LAN IPv4 addresses
// enumerated via net.Interfaces(). The certificate and key are written to
// certDir as cert.pem and key.pem.
//
// Trust-on-first-use: if both cert files already exist, they are reused
// as-is and never overwritten. This prevents silently replacing a cert that
// devices may have already trusted.
//
// The host parameter is included as an additional DNS name in the SAN list
// when it is not empty and not a wildcard ("0.0.0.0").
func EnsureSelfSignedCert(certDir, host string) (certPath, keyPath string, err error) {
	if err := os.MkdirAll(certDir, certDirPerm); err != nil {
		return "", "", fmt.Errorf("create cert dir: %w", err)
	}

	certPath = filepath.Join(certDir, "cert.pem")
	keyPath = filepath.Join(certDir, "key.pem")

	// Trust-on-first-use: reuse existing cert + key if both are present.
	if fileExists(certPath) && fileExists(keyPath) {
		log.Printf("TLS: reusing existing self-signed cert in %s", certDir)
		return certPath, keyPath, nil
	}

	// Generate an ECDSA P-256 private key.
	privKey, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		return "", "", fmt.Errorf("generate key: %w", err)
	}

	// Build the SAN list: localhost + 127.0.0.1 + LAN IPs + optional host.
	serial, err := rand.Int(rand.Reader, new(big.Int).Lsh(big.NewInt(1), 128))
	if err != nil {
		return "", "", fmt.Errorf("generate serial: %w", err)
	}

	dnsNames, ipAddrs := buildSANs(host)

	template := x509.Certificate{
		SerialNumber: serial,
		Subject: pkix.Name{
			Organization: []string{"Local Agent Interface"},
			CommonName:   "local-agent",
		},
		NotBefore:             time.Now().Add(-time.Minute),
		NotAfter:              time.Now().Add(certValidity),
		KeyUsage:              x509.KeyUsageDigitalSignature | x509.KeyUsageKeyEncipherment,
		ExtKeyUsage:           []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth, x509.ExtKeyUsageClientAuth},
		BasicConstraintsValid: true,
		DNSNames:              dnsNames,
		IPAddresses:           ipAddrs,
	}

	// Self-sign the certificate.
	derBytes, err := x509.CreateCertificate(rand.Reader, &template, &template, &privKey.PublicKey, privKey)
	if err != nil {
		return "", "", fmt.Errorf("create certificate: %w", err)
	}

	// Write the certificate (public, PEM-encoded).
	certPEM := pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: derBytes})
	if err := os.WriteFile(certPath, certPEM, certFilePerm); err != nil {
		return "", "", fmt.Errorf("write cert: %w", err)
	}

	// Write the private key (PEM-encoded ECDSA).
	keyDER, err := x509.MarshalECPrivateKey(privKey)
	if err != nil {
		return "", "", fmt.Errorf("marshal key: %w", err)
	}
	keyPEM := pem.EncodeToMemory(&pem.Block{Type: "EC PRIVATE KEY", Bytes: keyDER})
	if err := os.WriteFile(keyPath, keyPEM, certFilePerm); err != nil {
		return "", "", fmt.Errorf("write key: %w", err)
	}

	log.Printf("TLS: generated self-signed cert in %s (SANs: %d DNS, %d IPs)", certDir, len(dnsNames), len(ipAddrs))
	return certPath, keyPath, nil
}

// buildSANs collects DNS names and IP addresses for the certificate's
// Subject Alternative Name extension. It always includes "localhost" and
// 127.0.0.1, plus all non-loopback IPv4 addresses found on the host's
// network interfaces. If host is non-empty and not "0.0.0.0", it is added
// as a DNS name as well (useful when a hostname is configured).
func buildSANs(host string) (dnsNames []string, ipAddrs []net.IP) {
	dnsNames = []string{"localhost"}
	ipAddrs = []net.IP{net.ParseIP("127.0.0.1")}

	// Add the configured host as a DNS name if it is meaningful.
	if host != "" && host != "0.0.0.0" {
		// If host is an IP, add it to IPs; otherwise treat as a DNS name.
		if ip := net.ParseIP(host); ip != nil {
			ipAddrs = appendUniqueIP(ipAddrs, ip)
		} else {
			dnsNames = append(dnsNames, host)
		}
	}

	// Enumerate LAN IPv4 addresses from all network interfaces.
	ifaces, err := net.Interfaces()
	if err != nil {
		log.Printf("TLS: enumerate interfaces: %v", err)
		return
	}
	for _, iface := range ifaces {
		// Skip down interfaces.
		if iface.Flags&net.FlagUp == 0 {
			continue
		}
		addrs, err := iface.Addrs()
		if err != nil {
			continue
		}
		for _, addr := range addrs {
			var ip net.IP
			switch v := addr.(type) {
			case *net.IPNet:
				ip = v.IP
			case *net.IPAddr:
				ip = v.IP
			}
			// Only include non-loopback IPv4 addresses.
			if ip == nil || ip.IsLoopback() {
				continue
			}
			if ip.To4() != nil {
				ipAddrs = appendUniqueIP(ipAddrs, ip)
			}
		}
	}
	return
}

// appendUniqueIP appends ip to ips only if it is not already present.
func appendUniqueIP(ips []net.IP, ip net.IP) []net.IP {
	for _, existing := range ips {
		if existing.Equal(ip) {
			return ips
		}
	}
	return append(ips, ip)
}

// fileExists reports whether a file exists at path (not a directory).
func fileExists(path string) bool {
	info, err := os.Stat(path)
	if err != nil {
		return false
	}
	return !info.IsDir()
}
