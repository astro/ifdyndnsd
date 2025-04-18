{ nixosTest, writeText }:

let
  name = "client.example.net";
  alg = "hmac-sha256";
  secret-base64 = "wty5ZdU1eyeqOyW/j2CcnG2j7Hci4e3Qv+PAP4vdZto=";

  zonePath = "/var/lib/knot/master/example.net";

  server4 = "192.168.0.1";
  server6 = "fd00::1";
in
nixosTest {
  name = "ifdyndnsd";

  nodes.server = {
    networking.interfaces.eth1 = {
      ipv4.addresses = [ {
        address = server4;
        prefixLength = 24;
      } ];
      ipv6.addresses = [ {
        address = server6;
        prefixLength = 64;
      } ];
    };
    networking.firewall.allowedUDPPorts = [ 53 ];

    systemd.tmpfiles.settings."10-ifdyndnsd".${zonePath}.f = {
      mode = "0755";
      user = "knot";
      group = "knot";
      argument = "example.net. IN SOA . . ( 1 300 3600 7200 3600 )";
    };

    services.knot = {
      enable = true;

      keyFiles = [ (writeText "client-key.yaml" ''
        key:
          - id: "${name}"
            algorithm: "${alg}"
            secret: "${secret-base64}"
      '') ];

      settings = {
        server.listen = [ server4 server6 ];
        acl = [ {
          id = "client-update";
          key = [ "client.example.net" ];
          action = [ "update" ];
        } ];
        zone = [ {
          domain = "example.net";
          acl = [ "client-update" ];
          file = zonePath;
        } ];
      };
    };
  };

  nodes.client = { pkgs, ... }: {
    imports = [ ./nixos-module.nix ];

    networking.interfaces.eth1 = {
      ipv4.addresses = [ {
        address = "192.168.0.2";
        prefixLength = 24;
      } ];
      ipv6.addresses = [ {
        address = "fd00::2";
        prefixLength = 64;
      } ];
    };

    services.ifdyndnsd = {
      enable = true;
      logLevel = "trace";
      config = {
        keys."key" = {
          inherit name alg secret-base64;
          server = server4;
        };
        a = [ {
          key = "key";
          zone = "example.net";
          name = "client.example.net";
          interface = "dummy0";
          scope = "127.0.1.0/24";
        } ];
        aaaa = [ {
          key = "key";
          zone = "example.net";
          name = "client.example.net";
          interface = "dummy0";
          scope = "2000::/3";
        } ];
      };
    };

    environment.systemPackages = with pkgs; [
      # For `khost`
      knot-dns
    ];
  };

  testScript = ''
    import re

    start_all()
    server.wait_for_unit("knot.service")
    client.wait_for_unit("ifdyndnsd.service")
    client.succeed("ip link add dummy0 type dummy")
    client.succeed("ip link set dummy0 up")

    def test(query_type, query, pattern):
        out = client.succeed(f"khost -t {query_type} {query} ${server4}").strip()
        client.log(f"${server4} replied with: {out}")
        assert re.search(pattern, out), f'Did not match "{pattern}"'

    with subtest("No address yet"):
          test("A", "${name}", r"NXDOMAIN$")
          test("AAAA", "${name}", r"NXDOMAIN$")

    with subtest("Got an address"):
          client.succeed("ip addr add 127.0.1.1/8 dev dummy0")
          client.succeed("ip addr add 2001:db8::1/64 dev dummy0")
          client.sleep(2)
          test("A", "${name}", r"address 127.0.1.1$")
          test("AAAA", "${name}", r"address 2001:db8::1$")

    with subtest("Got an updated address"):
          client.succeed("ip addr add 127.0.1.2/8 dev dummy0")
          client.succeed("ip addr add 2001:db8::2/64 dev dummy0")
          client.succeed("ip addr del 127.0.1.1/8 dev dummy0")
          client.succeed("ip addr del 2001:db8::1/64 dev dummy0")
          client.sleep(2)
          test("A", "${name}", r"address 127.0.1.2$")
          test("AAAA", "${name}", r"address 2001:db8::2$")
  '';
}
