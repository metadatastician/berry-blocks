Pick your operating system. On GitHub's own wiki these are three ordinary code
blocks; in the lab they become one example.

```bash variant=macOS group=os persist label="Operating system"
brew install {{ package = ripgrep }}
```

```powershell variant=Windows group=os
winget install {{ package = ripgrep }}
```

```bash variant=Linux group=os
sudo apt-get install {{ package = ripgrep }}
```

Then check it worked with `rg --version`.
