```bash variant=macOS group=os persist label="Operating system"
brew upgrade {{ package = ripgrep }}
```

```powershell variant=Windows group=os
winget upgrade {{ package = ripgrep }}
```

```bash variant=Linux group=os
sudo apt-get install --only-upgrade {{ package = ripgrep }}
```
