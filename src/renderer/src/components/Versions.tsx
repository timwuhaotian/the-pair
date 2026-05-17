import { useState, useEffect } from 'react'
import { getVersion } from '@tauri-apps/api/app'

function Versions(): React.JSX.Element {
  const [version, setVersion] = useState<string>('…')

  useEffect(() => {
    void getVersion().then(setVersion)
  }, [])

  return (
    <ul className="font-mono text-[11px] text-muted-foreground space-y-0.5">
      <li>· the-pair v{version}</li>
      <li>· tauri v2.0</li>
    </ul>
  )
}

export default Versions
