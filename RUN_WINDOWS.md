# Windows 11 실행 안내

이 ZIP에는 `Bookcheckbot.exe`와 사용 설명서가 포함됩니다. **Rust와 Visual Studio 설치 없이 실행 가능합니다.**

1. ZIP 파일을 다운로드하고 압축을 풉니다.
2. 캡처할 콘텐츠를 브라우저에 띄우고 첫 페이지로 이동합니다.
3. 압축을 푼 폴더에서 **마우스 오른쪽 버튼 → 터미널에서 열기**로 PowerShell을 실행합니다.
4. 아래 명령으로 3페이지를 먼저 시험하세요.

```powershell
.\Bookcheckbot.exe --pages 3 --out test
```

이 명령은 **전체 모니터**를 캡처합니다. 저장 범위를 제한하려면 페이지 좌표를 지정하세요.

```powershell
.\Bookcheckbot.exe --pages 3 --out test --x 210 --y 130 --w 1240 --h 970
```

책의 모든 페이지를 저장하는 예시(총 458페이지):

```powershell
.\Bookcheckbot.exe --pages 458 --out book1 --x 210 --y 130 --w 1240 --h 970
```

151페이지부터 이어서 저장하려면 먼저 뷰어를 직접 151페이지로 이동하세요.

```powershell
.\Bookcheckbot.exe --pages 458 --start 151 --out book1 --x 210 --y 130 --w 1240 --h 970
```

마우스 클릭으로 넘기려면:

```powershell
.\Bookcheckbot.exe --pages 3 --turn click --click-x 1750 --click-y 950
```

캡처 중 키보드와 마우스를 건드리지 말고, 이상이 있으면 터미널에서 `Ctrl+C`로 중단하세요.

**주의**: 더블클릭하면 실행 옵션을 전달할 수 없으므로 바로 종료될 수 있습니다. 위와 같이 PowerShell에서 실행하세요. 파일은 코드 서명이 없기 때문에 Windows SmartScreen 경고가 나타날 수 있습니다. 출처와 파일 해시를 확인하고 신뢰할 때만 실행하세요. 캡처 제한 또는 DRM을 우회하는 기능은 없으며, 허용된 콘텐츠에만 사용해야 합니다. 자세한 좌표 확인·오류 해결은 `README.md`를 참조하세요.
