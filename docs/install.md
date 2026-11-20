# Install SoundTime from the terminal

You can run the installer on the computer that will host your library or on another computer that you use to manage it. The quickest way to start is to copy the command for your terminal from the README. To keep a copy of the installer, download **SoundTime-CLI.zip**, extract the entire archive and keep the `bin` folder alongside the launchers. The archive includes native executables for Windows, macOS and Linux on Intel/AMD 64-bit and ARM64, including Apple silicon. The installer uses these executables for both local and SSH installations. You do not need to install Python. [Download and run the CLI](../README.md#guided-installation) · [Install manually with Docker Compose](../README.md#manual-installation)

For a NAS LAN export from a Python source checkout, run `python -m pip install -r installer/requirements.txt` in your virtual environment before using `python installer/launcher.py export`. The portable CLI bundles this dependency.

## Start the installer

If you downloaded the ZIP, open a terminal in the extracted folder and run the command for your system.

| Terminal | Command |
| --- | --- |
| macOS or Linux terminal | `./soundtime` |
| Windows PowerShell | `.\soundtime.cmd` |
| Windows Command Prompt | `soundtime.cmd` |

The installer runs in English. Enter the number beside a menu option. Press Enter to accept the suggested value.

## Install on this computer

1. Choose **This computer**. The installer detects your system and suggests settings for this machine.
2. Choose **Recommended settings** for a new private installation. Choose **Customize** to change the installation folder, connect an existing music folder, select another web port, allow access from your local network or use S3 storage.
3. Review your choices. The installer checks the destination folder, ports and Docker before starting the services. On macOS or Windows, start Docker Desktop with Linux containers. On Linux, use Docker Engine with the Compose plugin. The guided installation requires Docker Compose 2.24.4 or later. [Docker Desktop](https://docs.docker.com/desktop/) · [Docker Compose plugin for Linux](https://docs.docker.com/compose/install/linux/)
4. Confirm the installation. When prompted, create your SoundTime administrator account. Your password stays hidden as you type. If you chose local network access, the installer offers to enable it after you create the account.
5. Open the address printed in the terminal and bookmark it. Docker keeps SoundTime running after you close the terminal.

## Install on another machine over SSH

Use this option for another Mac or PC, a dedicated server, a virtual private server (VPS), a virtual machine (VM) or a cloud instance. The host needs persistent storage and a running Docker Engine with Compose. If the host uses Docker Desktop, start it before connecting. The Linux executables require glibc 2.35 or later. If your NAS has its own container manager, you can [install from an exported project](#install-on-a-nas).

1. Enable SSH on the host and choose an account with permission to run Docker. Set up an SSH key that works from your terminal. If the key has a passphrase, load it into your SSH agent. On the first connection, check the host fingerprint against the one supplied by your administrator or hosting provider.
2. Choose **Another machine**, then **Install through SSH**. Enter the hostname or IP address, SSH username and port (usually 22). The installer detects the remote operating system and processor, then transfers the matching executable to a temporary folder. Enter installation and music folder paths as they appear on the remote host.
3. Confirm your settings and create the administrator account when prompted. For private access, the installer prints an SSH tunnel command. Run that command to reconnect after closing the installer.
4. To make SoundTime accessible over the internet, configure HTTPS and the firewall on your server or in your cloud provider’s control panel. [HTTPS deployment guide](deployment.md)

## Install on a NAS

Use a NAS with an x86-64 or ARM64 processor and a Linux container manager, such as Synology Container Manager or QNAP Container Station.

1. Choose **Another machine → Prepare an archive for a NAS without SSH**, then **Synology**, **QNAP** or **Other NAS with Docker**. This selects the suggested project folder for your NAS.
2. Choose a dedicated project folder **on the NAS**. To listen from other devices on your home network, use **Local network after setup**. The NAS project protects first-time network access with a temporary password.
3. Save the project ZIP and extract it into that exact folder on the NAS. Copy the hidden `.env` file, the `docker/` folder, `README.txt` and all other files from the archive. Keep the ZIP and the extracted project on a private share. They contain credentials for this installation.

### Synology Container Manager

1. In **File Station**, create the folder you chose, for example `/volume1/docker/soundtime`, and put the extracted project there. Check that `compose.yaml`, `.env` and `docker/` are in that folder and that `docker/nginx.conf` exists. The containers read these files from the absolute path you entered in the installer.
2. In **Container Manager → Project → Create**, name the project and select that same folder as its working directory. Use the contents of `compose.yaml` for the YAML configuration. If the file picker expects `docker-compose.yml`, give it a copy under that name. The Web Station portal is optional. Finish the DSM wizard and start the project. [Synology Container Manager projects](https://kb.synology.com/en-global/DSM/help/ContainerManager/docker_project)
3. If your version of Container Manager rejects the configuration, use the [Compose commands below](#start-the-extracted-project-with-compose) from the same folder. They tell Docker exactly where to find `.env`. [Environment variables in Docker Compose](https://docs.docker.com/compose/how-tos/environment-variables/variable-interpolation/)

### QNAP Container Station 3

1. Place the extracted project in the exact folder you chose, for example `/share/Container/soundtime`. Keep both the `docker/` and `qnap/` subfolders. The project uses absolute paths to files on the NAS. The private `qnap/` folder holds the settings for the SoundTime server and PostgreSQL.
2. Open `qnap-compose.yaml` in a text editor. In **Container Station → Applications → Create**, name the application, paste the **entire file**, click **Validate**, then create the application. Keep that name when managing the installation later. The pasted file tells Container Station where to find its private settings. [QNAP Container Station 3 applications](https://www.qnap.com/en-us/how-to/tutorial/article/how-to-use-container-station-3)
3. If validation reports a missing file or a permissions error, check the path and confirm that Container Station can read the project subfolders. If your version cannot read those files from the host, start the standard Compose project from its folder using the commands below.

### Portainer on Docker Standalone

Copy the extracted project to the folder you chose on the Docker host. In **Stacks → Add stack → Upload**, enter the stack name from the exported `README.txt` and upload `compose.yaml`. Under **Environment variables**, choose **Load variables from .env file** and select the project’s `.env`. Choose **Deploy the stack**. The host reads `docker/` from its original path when the containers start.

### Start the extracted project with Compose

Sign in to the host with an account that can run Docker. In a terminal, change to the extracted project folder and run these commands. The first command checks the configuration without printing secrets.

```sh
docker compose --env-file .env -f compose.yaml config --quiet
docker compose --env-file .env -f compose.yaml up -d
docker compose --env-file .env -f compose.yaml ps
```

Downloading the images and preparing the database can take a few minutes. Open the project’s `README.txt` for your chosen port and instructions. If the port is already in use, export a new project with another port before you create accounts or import music. When resuming an existing installation, keep its project folder, project name, credentials and volumes.

### Create the first account on the NAS

If you enabled local network access, open `http://NAS-IP:8880/setup` from a computer on your trusted network. Replace `NAS-IP` with the NAS address and use your chosen port if it differs from `8880`. When the browser prompts you, enter the temporary username `soundtime` and the password from the exported `README.txt`. Then create your own SoundTime administrator account, finish setup and check that you can sign in to your library.

In the project folder, rename `docker/access/bootstrap.active` to `docker/access/bootstrap.done`. Restart the `nginx` service in the NAS interface, or run `docker compose --env-file .env -f compose.yaml restart nginx` from the project folder. People on your network can now use SoundTime with the privacy settings you chose. Configure HTTPS and the firewall before allowing access from the internet.

If this installation came from an older export containing `docker/acces/bootstrap.active`, follow the guide shipped with that project. Rename that file to `docker/acces/bootstrap.termine` and restart the same service. The guide in a French-language export is named `LISEZ-MOI.txt`.

## Choose where to store audio

### Use an existing music folder

Choose **Customize → Audio storage → Disk on the target machine** and enter the folder containing your albums. SoundTime mounts this folder read-only. After creating your account, open **Admin**, choose **Synchronization** under **Storage**, then select **Run synchronization** to add the tracks to your catalog. The original files stay in place.

If your collection is on an external drive, connect it before starting SoundTime. Keep it connected during synchronization and playback. If Docker Desktop reports a mount permission error, allow the folder or drive in its file sharing settings and run the installer check again.

For music on a NAS, enter the shared folder path as the Docker host sees it, such as `/volume1/music` or `/share/Music`. If you access the same share through a Windows drive letter on another computer, enter the NAS path in the installer. For a Linux host that reads music over SMB or NFS, mount the share on the host first. [Read-only SMB and NFS music libraries](deployment.md#read-only-smb-and-nfs-music-libraries)

### Use Amazon S3 or an S3-compatible service

Choose **Customize → Audio storage → S3-compatible bucket** to store audio managed by SoundTime in an existing private bucket. The server, database and Docker volumes stay on the host. A dedicated bucket or prefix makes it easier to find and back up your audio.

| Setting | What to enter |
| --- | --- |
| Bucket | The exact bucket name, for example `my-private-music`. |
| Region | The AWS region of your bucket, such as `eu-west-3`, or the region specified by your S3-compatible provider. |
| Endpoint | For AWS, leave this empty and SoundTime uses the endpoint for your region. For another provider, enter its S3 API URL. That URL must be reachable from the SoundTime container. |
| Prefix | An optional folder-like prefix, such as `soundtime`, to store audio under `soundtime/`. |
| Access key and secret key | Credentials dedicated to SoundTime with access to the chosen bucket and prefix. |

SoundTime uses `s3:ListBucket` to list files and `s3:GetObject`, `s3:PutObject` and `s3:DeleteObject` to read, add and delete objects. Scope listing and object permissions to the chosen prefix. If you use a customer-managed AWS KMS key, your administrator may also need to grant access to the key. [S3 permissions by operation](https://docs.aws.amazon.com/AmazonS3/latest/userguide/using-with-s3-policy-actions.html) · [AWS regional endpoints](https://docs.aws.amazon.com/general/latest/gr/s3.html) · [S3 prefixes](https://docs.aws.amazon.com/AmazonS3/latest/userguide/using-prefixes.html)

The installer checks the S3 settings you entered. After installation, upload a small audio file and play it in SoundTime to verify write and read access. Keep track of the bucket, prefix and keys used by this project. Compose exports configured for S3 also contain the storage credentials.

## Commands for servers and automation

On Windows, replace `./soundtime` with `.\soundtime.cmd` in PowerShell or `soundtime.cmd` in Command Prompt. Add `--help` after a command to see its options.

```sh
./soundtime check
./soundtime install --directory "/srv/soundtime" --port 8880
./soundtime setup --port 8880 --admin-user admin --admin-email admin@example.org
./soundtime check --target auto --ssh-host music.example.org --ssh-user deploy
./soundtime export --target nas --directory "/volume1/docker/soundtime" --output soundtime-nas.zip
```

Use `--json` with a command to write the result to standard output and progress to standard error. An exit code of zero means the command succeeded. In an interactive terminal the installer prompts for passwords and keys without echoing them. For unattended installations, use your secret manager to set `SOUNDTIME_ADMIN_PASSWORD` for the first administrator account, or `SOUNDTIME_S3_ACCESS_KEY` and `SOUNDTIME_S3_SECRET_KEY` for S3 storage.

## Troubleshooting

| If you see this | What to do |
| --- | --- |
| Docker is unavailable or is running the wrong type of container | Start Docker on the destination machine. On Windows, configure Docker Desktop to use Linux containers. Run `check` again. |
| SSH connection failed | Check the username, port and server fingerprint. Try `ssh -p 22 user@host` in the same terminal, using your own connection details. Check that the correct key is available to SSH. |
| A port is in use or a folder is inaccessible | Choose an available port or a folder the account running Docker can access. When resuming an installation, use the existing project folder. |
| The launcher cannot find its executable | Extract the entire ZIP again, keeping the launcher and the six platform subfolders under `bin/` together. |
| Windows or macOS warns about the publisher | Download the official release and verify its SHA-256 checksum. The Windows executables are distributed without a publisher certificate, and the macOS executables are distributed without Apple notarization. [Apple’s guide to opening downloaded software](https://support.apple.com/en-us/102445) |

The installation folder contains the configuration and private `.env` file. Accounts and the catalog live in the project’s Docker volumes. To back up your instance, save the configuration, volumes, any S3 bucket and the original files in your external collection. [Backup instructions](deployment.md#backups)
