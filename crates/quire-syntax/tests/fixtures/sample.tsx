import React from "react";

type Props = { label: string; onClick?: () => void };

export function Button({ label, onClick }: Props): JSX.Element {
  const [n, setN] = React.useState<number>(0);
  return (
    <button className="btn" onClick={() => setN(n + 1)}>
      {label} {n}
      <Icon name="plus" />
    </button>
  );
}
