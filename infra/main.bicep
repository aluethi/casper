// ═══════════════════════════════════════════════════════════════════
// Casper Platform — Azure VM Deployment (behind Front Door)
// ═══════════════════════════════════════════════════════════════════
//
// Deploys a single VM running Docker Compose (casper-server, PostgreSQL,
// SearXNG, Caddy reverse proxy) plus an ACR for the Docker image.
// The VM is locked down to only accept traffic from Azure Front Door.
//
// Usage:
//   az group create -n rg-casper -l switzerlandnorth
//   az deployment group create -g rg-casper -f infra/main.bicep \
//     -p sshPublicKey="$(cat ~/.ssh/casper-deploy.pub)" \
//        frontDoorId="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
//
// The GitHub Actions workflow (deploy.yml) automates this.

targetScope = 'resourceGroup'

// ── Parameters ──────────────────────────────────────────────────

@description('Azure region')
param location string = resourceGroup().location

@description('SSH public key for VM access')
param sshPublicKey string

@description('Front Door instance ID (for X-Azure-FDID header validation)')
param frontDoorId string

@description('VM admin username')
param adminUsername string = 'casperadmin'

@description('VM size')
param vmSize string = 'Standard_B2as_v2'

// ── Variables ───────────────────────────────────────────────────

var uniqueSuffix = uniqueString(resourceGroup().id)
var acrName = 'acrcasper${uniqueSuffix}'
var vmName = 'vm-casper'
var dnsLabel = 'casper-${uniqueSuffix}'

var cloudInit = format('''
#cloud-config
package_update: true
packages:
  - ca-certificates
  - curl
  - gnupg

runcmd:
  # Install Docker Engine (official repo)
  - install -m 0755 -d /etc/apt/keyrings
  - curl -fsSL https://download.docker.com/linux/ubuntu/gpg -o /etc/apt/keyrings/docker.asc
  - chmod a+r /etc/apt/keyrings/docker.asc
  - echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/ubuntu $(lsb_release -cs) stable" | tee /etc/apt/sources.list.d/docker.list
  - apt-get update
  - apt-get install -y docker-ce docker-ce-cli containerd.io docker-compose-plugin
  - systemctl enable docker
  - usermod -aG docker {0}
  - mkdir -p /opt/casper
  - chown {0}:{0} /opt/casper
''', adminUsername)

// ── Networking ──────────────────────────────────────────────────

resource vnet 'Microsoft.Network/virtualNetworks@2024-01-01' = {
  name: 'vnet-casper'
  location: location
  properties: {
    addressSpace: { addressPrefixes: ['10.0.0.0/16'] }
    subnets: [
      {
        name: 'default'
        properties: { addressPrefix: '10.0.0.0/24' }
      }
    ]
  }
}

resource nsg 'Microsoft.Network/networkSecurityGroups@2024-01-01' = {
  name: 'nsg-casper'
  location: location
  properties: {
    securityRules: [
      {
        name: 'SSH'
        properties: {
          priority: 100
          direction: 'Inbound'
          access: 'Allow'
          protocol: 'Tcp'
          sourceAddressPrefix: '*'
          sourcePortRange: '*'
          destinationAddressPrefix: '*'
          destinationPortRange: '22'
        }
      }
      {
        name: 'HTTP-from-FrontDoor'
        properties: {
          priority: 200
          direction: 'Inbound'
          access: 'Allow'
          protocol: 'Tcp'
          sourceAddressPrefix: 'AzureFrontDoor.Backend'
          sourcePortRange: '*'
          destinationAddressPrefix: '*'
          destinationPortRange: '80'
        }
      }
      {
        name: 'DenyHTTP-Direct'
        properties: {
          priority: 300
          direction: 'Inbound'
          access: 'Deny'
          protocol: 'Tcp'
          sourceAddressPrefix: '*'
          sourcePortRange: '*'
          destinationAddressPrefix: '*'
          destinationPortRange: '80'
        }
      }
      {
        name: 'DenyHTTPS-Direct'
        properties: {
          priority: 400
          direction: 'Inbound'
          access: 'Deny'
          protocol: 'Tcp'
          sourceAddressPrefix: '*'
          sourcePortRange: '*'
          destinationAddressPrefix: '*'
          destinationPortRange: '443'
        }
      }
    ]
  }
}

resource publicIp 'Microsoft.Network/publicIPAddresses@2024-01-01' = {
  name: 'pip-casper'
  location: location
  sku: { name: 'Standard' }
  properties: {
    publicIPAllocationMethod: 'Static'
    dnsSettings: { domainNameLabel: dnsLabel }
  }
}

resource nic 'Microsoft.Network/networkInterfaces@2024-01-01' = {
  name: 'nic-casper'
  location: location
  properties: {
    networkSecurityGroup: { id: nsg.id }
    ipConfigurations: [
      {
        name: 'ipconfig1'
        properties: {
          subnet: { id: vnet.properties.subnets[0].id }
          publicIPAddress: { id: publicIp.id }
          privateIPAllocationMethod: 'Dynamic'
        }
      }
    ]
  }
}

// ── Container Registry ──────────────────────────────────────────

resource acr 'Microsoft.ContainerRegistry/registries@2023-11-01-preview' = {
  name: acrName
  location: location
  sku: { name: 'Basic' }
  properties: { adminUserEnabled: true }
}

// ── Virtual Machine ─────────────────────────────────────────────

resource vm 'Microsoft.Compute/virtualMachines@2024-07-01' = {
  name: vmName
  location: location
  properties: {
    hardwareProfile: { vmSize: vmSize }
    osProfile: {
      computerName: vmName
      adminUsername: adminUsername
      customData: base64(cloudInit)
      linuxConfiguration: {
        disablePasswordAuthentication: true
        ssh: {
          publicKeys: [
            {
              path: '/home/${adminUsername}/.ssh/authorized_keys'
              keyData: sshPublicKey
            }
          ]
        }
      }
    }
    storageProfile: {
      imageReference: {
        publisher: 'Canonical'
        offer: 'ubuntu-24_04-lts'
        sku: 'server'
        version: 'latest'
      }
      osDisk: {
        createOption: 'FromImage'
        diskSizeGB: 64
        managedDisk: { storageAccountType: 'StandardSSD_LRS' }
      }
    }
    networkProfile: {
      networkInterfaces: [{ id: nic.id }]
    }
  }
}

// ── Outputs ─────────────────────────────────────────────────────

output acrName string = acr.name
output acrLoginServer string = acr.properties.loginServer
output vmPublicIp string = publicIp.properties.ipAddress
output vmFqdn string = publicIp.properties.dnsSettings.fqdn
output sshCommand string = 'ssh ${adminUsername}@${publicIp.properties.dnsSettings.fqdn}'
output frontDoorId string = frontDoorId
