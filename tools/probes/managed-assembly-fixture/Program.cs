using System;

namespace UnityEngine
{
    public class MonoBehaviour { }

    [AttributeUsage(AttributeTargets.Field)]
    public sealed class SerializeField : Attribute { }
}

namespace Game
{
    public abstract class AbstractController : UnityEngine.MonoBehaviour { }

    public class BaseController : UnityEngine.MonoBehaviour
    {
        public long inherited;
    }

    public sealed class HeroController : BaseController
    {
        public int health;

        [UnityEngine.SerializeField]
        private float speed;

        [NonSerialized]
        public int ignored;

        public static int staticValue;
        public readonly int locked;
    }
}
