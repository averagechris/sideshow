Output "/tmp/sideshow-vhs2/demo.gif"
Output "/tmp/sideshow-vhs2/demo.webm"

Set Shell "zsh"
Set Width 1600
Set Height 900
Set FontSize 30
Set Framerate 24
Set Theme {"name":"sideshow-terminal","black":"#07100d","red":"#ff6b7a","green":"#7dff9b","yellow":"#e6db74","blue":"#7aa2ff","magenta":"#d787ff","cyan":"#77e8d0","white":"#e2f2e9","brightBlack":"#41524a","brightRed":"#ff8fa0","brightGreen":"#a5ffbe","brightYellow":"#fff0a3","brightBlue":"#9bbcff","brightMagenta":"#e5a8ff","brightCyan":"#a2fff0","brightWhite":"#f6fff9","background":"#07100d","foreground":"#e2f2e9","cursor":"#7dff9b","selection":"#183327"}
Set Padding 24
Set TypingSpeed 34ms

Hide
Type "export PATH=/tmp/sideshow-vhs2/sideshow-build/bin:$PATH"
Enter
Type "rm -rf /tmp/sideshow-vhs2/scratch && mkdir -p /tmp/sideshow-vhs2/scratch && cd /tmp/sideshow-vhs2/scratch"
Enter
Type "clear"
Enter
Type "sideshow new demo --theme terminal"
Show

Sleep 120ms
Enter
Sleep 1050ms

Type "ls demo/slides/"
Enter
Sleep 1000ms

Type "sideshow check demo"
Enter
Sleep 1200ms

Type "sideshow build demo"
Enter
Sleep 1700ms

Type "ls -lh demo/dist/"
Enter
Sleep 2400ms
